/**************************************************************************
* rmd.rs
*
* Required minimum distributions from pre-tax accounts (401(k), 403(b),
* traditional IRA)
**************************************************************************/

use chrono::{Datelike, NaiveDate};

// distribution periods from the IRS Uniform Lifetime Table (Publication
// 590-B, Appendix B, Table III), for ages 72 through 120. The table is
// used for anyone whose spouse isn't more than 10 years younger.
const UNIFORM_LIFETIME_TABLE: [f64; 49] = [
    27.4, 26.5, 25.5, 24.6, 23.7, 22.9, 22.0, 21.1, 20.2, 19.4, // 72-81
    18.5, 17.7, 16.8, 16.0, 15.2, 14.4, 13.7, 12.9, 12.2, 11.5, // 82-91
    10.8, 10.1, 9.5, 8.9, 8.4, 7.8, 7.3, 6.8, 6.4, 6.0, // 92-101
    5.6, 5.2, 4.9, 4.6, 4.3, 4.1, 3.9, 3.7, 3.5, 3.4, // 102-111
    3.3, 3.1, 3.0, 2.9, 2.8, 2.7, 2.5, 2.3, 2.0, // 112-120
];
const FIRST_TABLE_AGE: i32 = 72;

// the age distributions start, under the SECURE 2.0 Act: 73 for people born
// 1951 through 1959, and 75 for people born in 1960 or later. People born
// earlier started at 70 1/2 or 72, and are past 73 now.
pub fn start_age(date_of_birth: &NaiveDate) -> i32 {
    if date_of_birth.year() >= 1960 {
        75
    } else {
        73
    }
}

// the required distribution for the given year, from the pre-tax balance
// at the end of the year before. Uses the age the owner reaches during the
// year. Zero before distributions start.
pub fn annual_distribution(balance: f64, date_of_birth: &NaiveDate, year: i32) -> f64 {
    let age = year - date_of_birth.year();
    if age < start_age(date_of_birth) || balance <= 0.0 {
        return 0.0;
    }
    let index = (age - FIRST_TABLE_AGE).min(UNIFORM_LIFETIME_TABLE.len() as i32 - 1);
    balance / UNIFORM_LIFETIME_TABLE[index as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn born(year: i32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, 6, 15).unwrap()
    }

    #[test]
    fn test_start_age() {
        assert_eq!(start_age(&born(1951)), 73);
        assert_eq!(start_age(&born(1959)), 73);
        assert_eq!(start_age(&born(1960)), 75);
        assert_eq!(start_age(&born(1970)), 75);
    }

    #[test]
    fn test_annual_distribution() {
        // born 1955: none at 72, then the table from 73
        assert_eq!(annual_distribution(265000.0, &born(1955), 2027), 0.0);
        assert!((annual_distribution(265000.0, &born(1955), 2028) - 10000.0).abs() < 1e-9);
        // born 1960: none until 75
        assert_eq!(annual_distribution(246000.0, &born(1960), 2034), 0.0);
        assert!((annual_distribution(246000.0, &born(1960), 2035) - 10000.0).abs() < 1e-9);
        // 90 and 120 and over
        assert!((annual_distribution(122000.0, &born(1960), 2050) - 10000.0).abs() < 1e-9);
        assert_eq!(annual_distribution(20000.0, &born(1940), 2060), 10000.0);
        assert_eq!(annual_distribution(20000.0, &born(1940), 2065), 10000.0);
        // nothing to distribute
        assert_eq!(annual_distribution(0.0, &born(1955), 2030), 0.0);
    }
}
