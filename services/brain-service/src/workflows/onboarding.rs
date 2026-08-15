//! Farmer Onboarding Workflow
//!
//! First message from a new number → onboarding flow.
//!
//! Steps:
//!   1. collect_info → ask name, location, crop type
//!   2. verify_phone → OTP or WhatsApp number confirmation
//!   3. register_farmer → platform-service (identity)
//!   4. register_farm → farm-service
//!   5. welcome_message → WhatsApp with getting-started guide
