/**************************************************************************
* utils.rs
*
* Utilities useful application-wide
**************************************************************************/

use chrono::{Months, NaiveDate};

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

// adds whole calendar years, so the result falls on the same month and day
// (Feb 29 becomes Feb 28 in non-leap years)
pub fn add_years(date: &NaiveDate, years: u32) -> NaiveDate {
    date.checked_add_months(Months::new(years * 12)).unwrap_or(*date)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn test_add_years_keeps_month_and_day() {
        assert_eq!(add_years(&date(1960, 9, 30), 67), date(2027, 9, 30));
        assert_eq!(add_years(&date(1964, 2, 29), 1), date(1965, 2, 28));
        assert_eq!(add_years(&date(1964, 2, 29), 4), date(1968, 2, 29));
    }

    #[test]
    fn test_age_on_birthday() {
        let dob = date(1960, 9, 30);
        assert_eq!(get_age(&dob, &date(2027, 9, 29)), 66);
        assert_eq!(get_age(&dob, &add_years(&dob, 67)), 67);
    }
}
