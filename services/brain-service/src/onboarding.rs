//! Deterministic WhatsApp onboarding.
//!
//! The LLM never writes identity or farm records. It only enters the normal
//! agent loop after this state machine has collected, validated, summarized,
//! and received explicit confirmation for the data.

use anyhow::{Context, Result};
use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::context::FarmerContext;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Draft {
    name: Option<String>,
    farm_name: Option<String>,
    crop_name: Option<String>,
    crop_type: Option<String>,
    variety: Option<String>,
    cultivation_system: Option<String>,
    cultivation_unit_count: Option<u32>,
    area_per_unit_hectares: Option<String>,
    planted_at: Option<NaiveDate>,
    area_hectares: Option<String>,
}

#[derive(Debug)]
struct Session {
    step: Step,
    draft: Draft,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Consent,
    Name,
    FarmName,
    CropName,
    Variety,
    PlantedAt,
    Area,
    Confirm,
}

impl Step {
    fn as_str(self) -> &'static str {
        match self {
            Self::Consent => "consent",
            Self::Name => "name",
            Self::FarmName => "farm_name",
            Self::CropName => "crop_name",
            Self::Variety => "variety",
            Self::PlantedAt => "planted_at",
            Self::Area => "area",
            Self::Confirm => "confirm",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "consent" => Ok(Self::Consent),
            "name" => Ok(Self::Name),
            "farm_name" => Ok(Self::FarmName),
            "crop_name" => Ok(Self::CropName),
            "variety" => Ok(Self::Variety),
            "planted_at" => Ok(Self::PlantedAt),
            "area" => Ok(Self::Area),
            "confirm" => Ok(Self::Confirm),
            _ => anyhow::bail!("database returned an invalid onboarding step"),
        }
    }
}

/// Canonical identity key used by both onboarding and normal context lookup.
pub fn normalize_phone(value: &str) -> Result<String> {
    let digits: String = value.chars().filter(char::is_ascii_digit).collect();
    anyhow::ensure!(
        (8..=15).contains(&digits.len()),
        "invalid WhatsApp phone number"
    );
    Ok(format!("+{digits}"))
}

/// Returns `Some(reply)` while onboarding owns the conversation, otherwise the
/// caller should continue through the normal agronomist agent loop.
pub async fn handle(
    pool: &shared_db::DbPool,
    context: &FarmerContext,
    phone: &str,
    message: &str,
    request_id: Uuid,
) -> Result<Option<String>> {
    if let Some(reply) = cached_response(pool, phone, request_id).await? {
        return Ok(Some(reply));
    }

    let mut session = load_session(pool, phone).await?;
    let normalized = normalize_answer(message);

    if session.is_some() && is_cancel(&normalized) {
        let reply = "Pendaftaran dibatalkan. Data draf tidak disimpan sebagai profil kebun. Ketik *DAFTAR* kapan saja untuk memulai lagi.".to_owned();
        cancel_session(pool, phone, request_id, &reply).await?;
        return Ok(Some(reply));
    }

    if session.is_none() {
        if context.crop.is_some() && !is_start_command(&normalized) {
            return Ok(None);
        }

        let (step, draft, reply) = initial_state(context, is_start_command(&normalized));
        save_session(pool, phone, step, &draft, request_id, &reply).await?;
        return Ok(Some(reply));
    }

    let mut session = session.take().expect("checked above");
    let (next_step, reply) = match session.step {
        Step::Consent => {
            if is_affirmative(&normalized) {
                (
                    Step::Name,
                    "Baik. Siapa nama yang ingin digunakan di AgriSense? Contoh: *Rafiq Alha*.".to_owned(),
                )
            } else if is_negative(&normalized) {
                let reply = "Baik, pendaftaran tidak dilanjutkan dan data profil belum dibuat. Ketik *DAFTAR* jika berubah pikiran.".to_owned();
                cancel_session(pool, phone, request_id, &reply).await?;
                return Ok(Some(reply));
            } else {
                (
                    Step::Consent,
                    "Untuk melanjutkan, balas *SETUJU*. Balas *BATAL* jika tidak ingin menyimpan profil dan data kebun.".to_owned(),
                )
            }
        }
        Step::Name => match clean_label(message, 2, 100) {
            Ok(value) => {
                session.draft.name = Some(value);
                (
                    Step::FarmName,
                    "Apa nama kebun Anda? Contoh: *Kebun Melon Sejahtera*.".to_owned(),
                )
            }
            Err(_) => (
                Step::Name,
                "Nama belum valid. Masukkan 2–100 karakter dan jangan sertakan nomor identitas, tautan, atau baris baru.".to_owned(),
            ),
        },
        Step::FarmName => match clean_label(message, 2, 120) {
            Ok(value) => {
                session.draft.farm_name = Some(value);
                (
                    Step::CropName,
                    "Tanaman aktifnya apa? Contoh: *melon*, *cabai*, *padi*, atau *jagung*.".to_owned(),
                )
            }
            Err(_) => (
                Step::FarmName,
                "Nama kebun belum valid. Masukkan 2–120 karakter tanpa tautan atau baris baru.".to_owned(),
            ),
        },
        Step::CropName => match clean_label(message, 2, 80) {
            Ok(value) => {
                let (crop_type, crop_name) = classify_crop(&value);
                let (cultivation_system, cultivation_unit_count) =
                    parse_cultivation_details(&value);
                session.draft.crop_type = Some(crop_type.to_owned());
                session.draft.crop_name = Some(crop_name);
                session.draft.cultivation_system = cultivation_system;
                session.draft.cultivation_unit_count = cultivation_unit_count;
                (
                    Step::Variety,
                    "Apa varietasnya? Contoh: *Golden Alisha* atau *Rawit Dewata*. Jika tidak tahu, balas *LEWATI*.".to_owned(),
                )
            }
            Err(_) => (
                Step::CropName,
                "Nama tanaman belum valid. Tulis nama tanamannya saja, misalnya *melon* atau *cabai*.".to_owned(),
            ),
        },
        Step::Variety => {
            if is_skip(&normalized) {
                session.draft.variety = None;
                (
                    Step::PlantedAt,
                    "Kapan tanggal tanamnya? Gunakan format *DD-MM-YYYY*, misalnya *10-08-2026*.".to_owned(),
                )
            } else {
                match clean_label(message, 2, 100) {
                    Ok(value) => {
                        session.draft.variety = Some(value);
                        (
                            Step::PlantedAt,
                            "Kapan tanggal tanamnya? Gunakan format *DD-MM-YYYY*, misalnya *10-08-2026*.".to_owned(),
                        )
                    }
                    Err(_) => (
                        Step::Variety,
                        "Varietas belum valid. Tulis nama varietasnya atau balas *LEWATI*.".to_owned(),
                    ),
                }
            }
        }
        Step::PlantedAt => match parse_planting_date(message, Utc::now().date_naive()) {
            Ok(date) => {
                session.draft.planted_at = Some(date);
                (
                    Step::Area,
                    "Berapa luas area tanaman? Sertakan satuan, misalnya *1000 m2* atau *0,5 ha*.".to_owned(),
                )
            }
            Err(_) => (
                Step::PlantedAt,
                "Tanggal belum valid. Gunakan *DD-MM-YYYY*, tidak boleh di masa depan, dan maksimal 10 tahun lalu.".to_owned(),
            ),
        },
        Step::Area => match parse_area_hectares(message, session.draft.cultivation_unit_count) {
            Ok(area) => {
                session.draft.area_hectares = Some(format_area_db(area.total_hectares));
                session.draft.area_per_unit_hectares =
                    area.per_unit_hectares.map(format_area_db);
                let reply = confirmation_summary(&session.draft)?;
                (Step::Confirm, reply)
            }
            Err(_) => (
                Step::Area,
                "Luas belum valid atau bukan luas total. Masukkan total seluruh area tanaman, misalnya *10000 m2* atau *1 ha*. Jangan masukkan luas per greenhouse/petak.".to_owned(),
            ),
        },
        Step::Confirm => {
            if is_affirmative(&normalized) {
                let reply = completion_reply(&session.draft)?;
                complete(pool, phone, &session.draft, request_id, &reply).await?;
                return Ok(Some(reply));
            }
            if is_negative(&normalized) || normalized == "ubah" {
                session.draft = Draft::default();
                (
                    Step::Name,
                    "Baik, kita isi ulang agar tidak ada data yang keliru. Siapa nama yang ingin digunakan?".to_owned(),
                )
            } else {
                (
                    Step::Confirm,
                    "Balas *YA* untuk menyimpan, *UBAH* untuk mengisi ulang, atau *BATAL* untuk membatalkan.".to_owned(),
                )
            }
        }
    };

    session.step = next_step;
    save_session(
        pool,
        phone,
        session.step,
        &session.draft,
        request_id,
        &reply,
    )
    .await?;
    Ok(Some(reply))
}

fn initial_state(context: &FarmerContext, start_requested: bool) -> (Step, Draft, String) {
    if context.user_id.is_none() {
        let reply = if start_requested {
            "Sebelum mulai, AgriSense perlu menyimpan nama, nomor WhatsApp, dan data kebun untuk memberi jawaban yang sesuai. Data dipisahkan per pengguna. Balas *SETUJU* untuk melanjutkan atau *BATAL*."
        } else {
            "Halo! Agar jawaban sesuai kondisi kebun Anda, AgriSense perlu membuat profil petani dan tanaman aktif. Data dipisahkan per pengguna. Balas *SETUJU* untuk mulai atau *BATAL*."
        };
        return (Step::Consent, Draft::default(), reply.to_owned());
    }

    let mut draft = Draft {
        name: context.farmer_name.clone(),
        farm_name: context.farm_name.clone(),
        ..Draft::default()
    };
    if draft.farm_name.is_none() {
        (
            Step::FarmName,
            draft,
            "Profil Anda sudah dikenali. Apa nama kebun yang ingin didaftarkan?".to_owned(),
        )
    } else {
        // Reset crop-specific fields when adding a new planting.
        draft.crop_name = None;
        (
            Step::CropName,
            draft,
            "Tanaman aktif apa yang ingin didaftarkan? Contoh: *melon*, *cabai*, atau *padi*."
                .to_owned(),
        )
    }
}

async fn load_session(pool: &shared_db::DbPool, phone: &str) -> Result<Option<Session>> {
    let mut tx = phone_transaction(pool, phone).await?;
    let row = sqlx::query("SELECT current_step, draft FROM ai.get_onboarding_session()")
        .fetch_optional(&mut *tx)
        .await?;
    tx.commit().await?;

    row.map(|row| {
        let step: String = row.try_get("current_step")?;
        let draft: serde_json::Value = row.try_get("draft")?;
        Ok(Session {
            step: Step::parse(&step)?,
            draft: serde_json::from_value(draft).context("invalid onboarding draft in database")?,
        })
    })
    .transpose()
}

async fn cached_response(
    pool: &shared_db::DbPool,
    phone: &str,
    request_id: Uuid,
) -> Result<Option<String>> {
    let mut tx = phone_transaction(pool, phone).await?;
    let response = sqlx::query_scalar("SELECT ai.get_onboarding_response($1)")
        .bind(request_id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(response)
}

async fn save_session(
    pool: &shared_db::DbPool,
    phone: &str,
    step: Step,
    draft: &Draft,
    request_id: Uuid,
    response: &str,
) -> Result<()> {
    let mut tx = phone_transaction(pool, phone).await?;
    sqlx::query("SELECT ai.save_onboarding_session($1, $2, $3, $4)")
        .bind(step.as_str())
        .bind(serde_json::to_value(draft)?)
        .bind(request_id)
        .bind(response)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

async fn cancel_session(
    pool: &shared_db::DbPool,
    phone: &str,
    request_id: Uuid,
    response: &str,
) -> Result<()> {
    let mut tx = phone_transaction(pool, phone).await?;
    sqlx::query("SELECT ai.cancel_onboarding_session($1, $2)")
        .bind(request_id)
        .bind(response)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

async fn complete(
    pool: &shared_db::DbPool,
    phone: &str,
    draft: &Draft,
    request_id: Uuid,
    response: &str,
) -> Result<()> {
    let name = required(&draft.name, "name")?;
    let farm_name = required(&draft.farm_name, "farm_name")?;
    let crop_name = required(&draft.crop_name, "crop_name")?;
    let crop_type = required(&draft.crop_type, "crop_type")?;
    let planted_at = draft.planted_at.context("missing planted_at")?;
    let area = required(&draft.area_hectares, "area_hectares")?;
    let seed_variety = stored_variety(crop_type, crop_name, draft.variety.as_deref());
    let cultivation_system = draft.cultivation_system.as_deref();
    let cultivation_unit_count = draft.cultivation_unit_count.map(i64::from);
    let area_per_unit = draft.area_per_unit_hectares.as_deref();

    let mut tx = phone_transaction(pool, phone).await?;
    sqlx::query(
        "SELECT * FROM ai.complete_whatsapp_onboarding($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(name)
    .bind(farm_name)
    .bind(crop_type)
    .bind(seed_variety)
    .bind(area)
    .bind(planted_at)
    .bind(request_id)
    .bind(response)
    .bind(cultivation_system)
    .bind(cultivation_unit_count)
    .bind(area_per_unit)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn phone_transaction<'a>(
    pool: &'a shared_db::DbPool,
    phone: &str,
) -> Result<sqlx::Transaction<'a, sqlx::Postgres>> {
    let normalized = normalize_phone(phone)?;
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.current_phone', $1, TRUE)")
        .bind(normalized)
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}

fn required<'a>(value: &'a Option<String>, field: &str) -> Result<&'a str> {
    value
        .as_deref()
        .with_context(|| format!("onboarding draft is missing {field}"))
}

fn clean_label(value: &str, min: usize, max: usize) -> Result<String> {
    let value = value.trim();
    let length = value.chars().count();
    anyhow::ensure!((min..=max).contains(&length), "invalid length");
    anyhow::ensure!(!value.chars().any(char::is_control), "control character");
    anyhow::ensure!(
        !value.to_ascii_lowercase().contains("http://")
            && !value.to_ascii_lowercase().contains("https://"),
        "links are not accepted"
    );
    Ok(value.to_owned())
}

fn normalize_answer(value: &str) -> String {
    value.trim().to_lowercase()
}

fn is_affirmative(value: &str) -> bool {
    matches!(
        value,
        "ya" | "iya" | "setuju" | "benar" | "simpan" | "ok" | "oke"
    )
}

fn is_negative(value: &str) -> bool {
    matches!(value, "tidak" | "nggak" | "enggak" | "jangan")
}

fn is_cancel(value: &str) -> bool {
    matches!(value, "batal" | "cancel" | "berhenti")
}

fn is_skip(value: &str) -> bool {
    matches!(value, "lewati" | "skip" | "tidak tahu" | "tidak ada")
}

fn is_start_command(value: &str) -> bool {
    matches!(
        value,
        "daftar" | "mulai daftar" | "daftar kebun" | "daftar tanaman" | "tambah tanaman"
    )
}

fn classify_crop(value: &str) -> (&'static str, String) {
    let lower = value.to_lowercase();
    let (crop_type, crop_name) = if lower.contains("padi") {
        ("rice", "Padi")
    } else if lower.contains("jagung") {
        ("corn", "Jagung")
    } else if lower.contains("kedelai") {
        ("soybean", "Kedelai")
    } else if lower.contains("tebu") {
        ("sugarcane", "Tebu")
    } else if lower.contains("singkong") || lower.contains("ubi kayu") {
        ("cassava", "Singkong")
    } else if lower.contains("tomat") {
        ("tomato", "Tomat")
    } else if lower.contains("cabai") || lower.contains("cabe") {
        ("chili", "Cabai")
    } else if lower.contains("kubis") || lower == "kol" {
        ("cabbage", "Kubis")
    } else if lower.contains("bawang merah") {
        ("shallot", "Bawang Merah")
    } else if lower.contains("melon") {
        ("melon", "Melon")
    } else {
        ("other", value.trim())
    };
    (crop_type, title_case(crop_name))
}

fn parse_cultivation_details(value: &str) -> (Option<String>, Option<u32>) {
    let lower = value.to_lowercase();
    let system = if lower.contains("greenhouse") || lower.contains("green house") {
        Some("greenhouse".to_owned())
    } else if lower.contains("screenhouse") || lower.contains("screen house") {
        Some("screen_house".to_owned())
    } else if lower.contains("hidroponik") || lower.contains("hydroponic") {
        Some("hydroponic".to_owned())
    } else {
        None
    };

    let count = system.as_ref().and_then(|_| {
        lower
            .split(|ch: char| !ch.is_ascii_digit())
            .find(|part| !part.is_empty())
            .and_then(|part| part.parse::<u32>().ok())
            .filter(|count| (1..=10_000).contains(count))
    });
    (system, count)
}

fn title_case(value: &str) -> String {
    value
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_planting_date(value: &str, today: NaiveDate) -> Result<NaiveDate> {
    let value = value.trim();
    let date = ["%d-%m-%Y", "%d/%m/%Y", "%Y-%m-%d"]
        .iter()
        .find_map(|format| NaiveDate::parse_from_str(value, format).ok())
        .context("invalid date")?;
    anyhow::ensure!(date <= today, "future date");
    anyhow::ensure!((today - date).num_days() <= 3_650, "date too old");
    Ok(date)
}

#[derive(Debug, PartialEq)]
struct ParsedArea {
    total_hectares: f64,
    per_unit_hectares: Option<f64>,
}

fn parse_area_hectares(value: &str, unit_count: Option<u32>) -> Result<ParsedArea> {
    let lower = value.trim().to_lowercase().replace('²', "2");
    let is_per_unit = lower.contains("per ")
        || lower.contains("masing-masing")
        || lower.contains("/greenhouse")
        || lower.contains("/petak");
    let is_square_meter = lower.contains("m2") || lower.contains("meter");
    let is_hectare = lower.contains("ha") || lower.contains("hektar");
    anyhow::ensure!(is_square_meter ^ is_hectare, "missing or ambiguous unit");

    let raw_number: String = lower
        .chars()
        .skip_while(|ch| !ch.is_ascii_digit())
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.' || *ch == ',')
        .collect();
    anyhow::ensure!(!raw_number.is_empty(), "missing number");

    let normalized_number = if raw_number.contains(',') {
        raw_number.replace('.', "").replace(',', ".")
    } else if is_square_meter
        && raw_number.matches('.').count() == 1
        && raw_number
            .split('.')
            .nth(1)
            .is_some_and(|part| part.len() == 3)
    {
        raw_number.replace('.', "")
    } else {
        raw_number
    };
    let amount: f64 = normalized_number.parse()?;
    anyhow::ensure!(amount.is_finite() && amount > 0.0, "invalid area");
    let entered_hectares = if is_square_meter {
        amount / 10_000.0
    } else {
        amount
    };
    let total_hectares = if is_per_unit {
        entered_hectares * f64::from(unit_count.context("missing cultivation unit count")?)
    } else {
        entered_hectares
    };
    anyhow::ensure!(total_hectares <= 100_000.0, "area too large");
    Ok(ParsedArea {
        total_hectares,
        per_unit_hectares: is_per_unit.then_some(entered_hectares),
    })
}

fn format_area_db(area: f64) -> String {
    format!("{area:.4}")
}

fn display_area(area: &str) -> Result<String> {
    let hectares: f64 = area.parse()?;
    if hectares < 1.0 {
        Ok(format!("{:.0} m²", hectares * 10_000.0))
    } else {
        let number = format!("{hectares:.4}");
        Ok(format!(
            "{} ha",
            number.trim_end_matches('0').trim_end_matches('.')
        ))
    }
}

fn crop_display(draft: &Draft) -> Result<String> {
    let crop = required(&draft.crop_name, "crop_name")?;
    Ok(match draft.variety.as_deref() {
        Some(variety) => format!("{crop} — varietas {variety}"),
        None => format!("{crop} — varietas belum diketahui"),
    })
}

fn cultivation_display(draft: &Draft) -> Result<String> {
    match (
        draft.cultivation_system.as_deref(),
        draft.cultivation_unit_count,
        draft.area_per_unit_hectares.as_deref(),
    ) {
        (Some(system), Some(count), Some(area)) => Ok(format!(
            "• Sistem: {count} {system} ({} per unit)\n",
            display_area(area)?
        )),
        (Some(system), Some(count), None) => Ok(format!("• Sistem: {count} {system}\n")),
        (Some(system), None, _) => Ok(format!("• Sistem: {system}\n")),
        (None, _, _) => Ok(String::new()),
    }
}

fn confirmation_summary(draft: &Draft) -> Result<String> {
    let date = draft.planted_at.context("missing planted_at")?;
    Ok(format!(
        "Mohon periksa data berikut:\n\n• Nama: {}\n• Kebun: {}\n• Tanaman: {}\n{}• Tanggal tanam: {}\n• Luas total: {}\n\nBalas *YA* untuk menyimpan, *UBAH* untuk mengisi ulang, atau *BATAL*.",
        required(&draft.name, "name")?,
        required(&draft.farm_name, "farm_name")?,
        crop_display(draft)?,
        cultivation_display(draft)?,
        date.format("%d-%m-%Y"),
        display_area(required(&draft.area_hectares, "area_hectares")?)?,
    ))
}

fn completion_reply(draft: &Draft) -> Result<String> {
    let date = draft.planted_at.context("missing planted_at")?;
    let age = (Utc::now().date_naive() - date).num_days();
    Ok(format!(
        "Data berhasil disimpan. 🌱\n\nTanaman aktif: {}\nKebun: {}\n{}Umur tanaman: {} hari\nLuas total: {}\n\nMulai sekarang, jawaban AgriSense akan menggunakan data tanaman ini sebagai konteks.",
        crop_display(draft)?,
        required(&draft.farm_name, "farm_name")?,
        cultivation_display(draft)?,
        age,
        display_area(required(&draft.area_hectares, "area_hectares")?)?,
    ))
}

fn stored_variety(crop_type: &str, crop_name: &str, variety: Option<&str>) -> String {
    if crop_type == "other" {
        match variety {
            Some(variety) => format!("{crop_name} ({variety})"),
            None => crop_name.to_owned(),
        }
    } else {
        variety.unwrap_or_default().to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phone_is_canonical_and_bounded() {
        assert_eq!(
            normalize_phone("+62 857-0947-7872").unwrap(),
            "+6285709477872"
        );
        assert!(normalize_phone("123").is_err());
    }

    #[test]
    fn crop_mapping_uses_first_class_melon_and_canonical_name() {
        assert_eq!(classify_crop("cabai rawit").0, "chili");
        let melon = classify_crop("melon, 10 greenhouse");
        assert_eq!(melon.0, "melon");
        assert_eq!(melon.1, "Melon");
        assert_eq!(
            stored_variety("melon", "Melon", Some("Golden Alisha")),
            "Golden Alisha"
        );
        assert_eq!(
            parse_cultivation_details("melon, 10 greenhouse"),
            (Some("greenhouse".into()), Some(10))
        );
    }

    #[test]
    fn area_requires_unit_and_converts_square_meters() {
        let total = parse_area_hectares("1.000 m2", None).unwrap();
        assert_eq!(total.total_hectares, 0.1);
        assert_eq!(total.per_unit_hectares, None);

        let per_greenhouse = parse_area_hectares("1000 m2 per greenhouse", Some(10)).unwrap();
        assert_eq!(per_greenhouse.total_hectares, 1.0);
        assert_eq!(per_greenhouse.per_unit_hectares, Some(0.1));

        assert_eq!(
            parse_area_hectares("0,5 hektar", None)
                .unwrap()
                .total_hectares,
            0.5
        );
        assert!(parse_area_hectares("1000 m2 per greenhouse", None).is_err());
        assert!(parse_area_hectares("1000", None).is_err());
        assert!(parse_area_hectares("0 ha", None).is_err());
    }

    #[test]
    fn planting_date_is_not_future_or_stale() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 5).unwrap();
        assert_eq!(
            parse_planting_date("10-08-2026", today).unwrap(),
            NaiveDate::from_ymd_opt(2026, 8, 10).unwrap()
        );
        assert!(parse_planting_date("10-10-2026", today).is_err());
        assert!(parse_planting_date("01-01-2000", today).is_err());
    }

    #[test]
    fn confirmation_contains_every_user_control() {
        let draft = Draft {
            name: Some("Rafiq".into()),
            farm_name: Some("Kebun Maju".into()),
            crop_name: Some("Melon".into()),
            crop_type: Some("melon".into()),
            variety: Some("Golden Alisha".into()),
            cultivation_system: Some("greenhouse".into()),
            cultivation_unit_count: Some(10),
            area_per_unit_hectares: Some("0.1000".into()),
            planted_at: Some(NaiveDate::from_ymd_opt(2026, 8, 10).unwrap()),
            area_hectares: Some("1.0000".into()),
        };
        let summary = confirmation_summary(&draft).unwrap();
        assert!(summary.contains("Rafiq"));
        assert!(summary.contains("Golden Alisha"));
        assert!(summary.contains("YA"));
        assert!(summary.contains("UBAH"));
        assert!(summary.contains("BATAL"));
    }
}
