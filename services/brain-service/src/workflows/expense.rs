//! Expense Recording Workflow
//!
//! "Beli pupuk 50kg Rp 200.000"
//!
//! Steps:
//!   1. parse_expense → extract amount, category, description
//!   2. save_transaction → finance-service
//!   3. update_cashflow → finance-service
//!   4. confirm_to_farmer → "Tercatat: pupuk Rp 200.000"
