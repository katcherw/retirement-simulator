/**************************************************************************
* utils.rs
*
* Utilities useful application-wide
**************************************************************************/

use chrono::{NaiveDate, Duration};

// given an annual interest rate, return the equivalent monthly rate. This
// allows the inputs to be entered in the familiar annual yield, but the
// simulation to be done by month.
pub fn get_monthly_rate(annual_rate: f32) -> f32 {
    // growth rates are expressed as rates compunded annually, but we will
    // calculate on a monthly basis
    (1.0 + annual_rate).powf(1.0 / 12.0) - 1.0
}

pub fn get_age(date_of_birth: &NaiveDate, current_date: &NaiveDate) -> u32 {
    let years_diff = current_date.years_since(*date_of_birth);
    match years_diff {
        Some(v) => v,
        None => 0,
    }
}

pub fn add_years(date: &NaiveDate, years: u32) -> NaiveDate {
    let days_to_add = years as i64 * 365;
    match date.checked_add_signed(Duration::days(days_to_add)) {
        Some(v) => v,
        None => *date,
    }
}
