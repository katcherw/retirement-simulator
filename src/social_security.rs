/**************************************************************************
* social_security.rs
*
* Social Security benefits for one or two retirees, from each retiree's
* primary insurance amount (PIA): the monthly benefit at full retirement age,
* as reported on the SSA web site.
*
* Each retiree can receive:
*  - their own retirement benefit, reduced for claiming before full
*    retirement age (FRA), or increased by delayed retirement credits for
*    claiming after it (up to 70).
*  - a spousal benefit while the other retiree is alive: up to half of the
*    spouse's PIA, minus their own PIA, reduced for claiming before FRA.
*    It starts when both have claimed, and has no delayed credits.
*  - a survivor benefit after the other retiree dies: based on what the
*    deceased was receiving (or was entitled to), reduced if the survivor
*    starts it before their survivor FRA. A survivor receives the larger of
*    their own benefit and the survivor benefit, not both.
*
* Ages are whole months. Benefits are in today's dollars, so cost of living
* adjustments are already accounted for.
**************************************************************************/

use chrono::{Datelike, Months, NaiveDate};

const EARLIEST_CLAIMING_AGE: u32 = 62 * 12;
// no delayed retirement credits are earned after 70
const LATEST_CREDIT_AGE: u32 = 70 * 12;
const EARLIEST_SURVIVOR_AGE: u32 = 60 * 12;
// a survivor starting at 60 receives 71.5% of the full survivor benefit
const MAX_SURVIVOR_REDUCTION: f64 = 0.285;
// if the deceased claimed early, the survivor receives at least this
// fraction of the deceased's PIA (the "widow(er)'s limit")
const SURVIVOR_MINIMUM_OF_PIA: f64 = 0.825;

// a retiree, as far as social security is concerned
#[derive(Debug, Clone)]
pub struct Person {
    pub date_of_birth: NaiveDate,
    // monthly benefit at full retirement age, in today's dollars
    pub pia: f64,
    // age in whole years when this retiree claims their own benefit (at
    // least 62)
    pub claiming_age: u32,
    // first day the retiree is no longer alive
    pub death_date: NaiveDate,
}

// SSA treats people born on January 1 as if born the year before
fn birth_year(date_of_birth: &NaiveDate) -> i32 {
    date_of_birth.pred_opt().unwrap_or(*date_of_birth).year()
}

// full retirement age in months, by year of birth
fn full_retirement_age_for_year(year: i32) -> u32 {
    match year {
        ..=1937 => 65 * 12,
        1938..=1942 => 65 * 12 + 2 * (year - 1937) as u32,
        1943..=1954 => 66 * 12,
        1955..=1959 => 66 * 12 + 2 * (year - 1954) as u32,
        1960.. => 67 * 12,
    }
}

pub fn full_retirement_age(date_of_birth: &NaiveDate) -> u32 {
    full_retirement_age_for_year(birth_year(date_of_birth))
}

// the full retirement age for survivor benefits follows the same steps as
// the retirement table, two years later
pub fn survivor_full_retirement_age(date_of_birth: &NaiveDate) -> u32 {
    full_retirement_age_for_year(birth_year(date_of_birth) - 2)
}

// delayed retirement credit per year after full retirement age, as a
// fraction. 8% for anyone born in 1943 or later, less for earlier years.
fn delayed_credit_per_year(date_of_birth: &NaiveDate) -> f64 {
    let year = birth_year(date_of_birth);
    if year < 1925 {
        0.03
    } else {
        f64::min(0.035 + 0.005 * ((year - 1925) / 2) as f64, 0.08)
    }
}

// age in whole months on the given date
fn age_in_months(date_of_birth: &NaiveDate, date: &NaiveDate) -> u32 {
    let months = (date.year() - date_of_birth.year()) * 12 + date.month() as i32
        - date_of_birth.month() as i32;
    if months <= 0 {
        return 0;
    }
    let months = months as u32;
    match date_of_birth.checked_add_months(Months::new(months)) {
        Some(anniversary) if anniversary > *date => months - 1,
        _ => months,
    }
}

fn add_months(date: &NaiveDate, months: u32) -> NaiveDate {
    date.checked_add_months(Months::new(months))
        .unwrap_or(NaiveDate::MAX)
}

// own retirement benefit as a fraction of PIA, for claiming at the given
// age in months. Before FRA: 5/9% less for each of the first 36 months
// early, 5/12% for each month beyond that. After FRA: the delayed credit
// for each month, up to age 70.
pub fn retirement_benefit_factor(date_of_birth: &NaiveDate, claiming_age: u32) -> f64 {
    let fra = full_retirement_age(date_of_birth);
    let claiming_age = claiming_age.max(EARLIEST_CLAIMING_AGE);
    if claiming_age < fra {
        let early = fra - claiming_age;
        1.0 - early_reduction(early, 5.0 / 9.0)
    } else {
        let late = claiming_age.min(LATEST_CREDIT_AGE).saturating_sub(fra);
        1.0 + delayed_credit_per_year(date_of_birth) / 12.0 * late as f64
    }
}

// spousal benefit as a fraction of the full spousal benefit, starting at
// the given age in months. Before FRA: 25/36% less for each of the first
// 36 months early, 5/12% for each month beyond that. No delayed credits.
fn spousal_benefit_factor(date_of_birth: &NaiveDate, age: u32) -> f64 {
    let fra = full_retirement_age(date_of_birth);
    1.0 - early_reduction(fra.saturating_sub(age), 25.0 / 36.0)
}

// reduction (fraction) for starting a benefit the given number of months
// early, with the given percentage for each of the first 36 months
fn early_reduction(months_early: u32, first_36_percent: f64) -> f64 {
    let first = months_early.min(36) as f64;
    let rest = months_early.saturating_sub(36) as f64;
    (first * first_36_percent + rest * 5.0 / 12.0) / 100.0
}

// survivor benefit as a fraction of the full survivor benefit, starting at
// the given age in months: reduced in equal steps from 100% at survivor FRA
// to 71.5% at 60
fn survivor_benefit_factor(date_of_birth: &NaiveDate, age: u32) -> f64 {
    let fra = survivor_full_retirement_age(date_of_birth);
    if age >= fra {
        return 1.0;
    }
    let age = age.max(EARLIEST_SURVIVOR_AGE);
    1.0 - MAX_SURVIVOR_REDUCTION * (fra - age) as f64 / (fra - EARLIEST_SURVIVOR_AGE) as f64
}

// the benefits for one retiree, worked out in advance: the dates and the
// life expectancies are known, so they don't change during a simulation
#[derive(Debug, Clone)]
struct RetireeBenefits {
    death_date: NaiveDate,
    own_start: NaiveDate,
    own: f64,
    // added to the own benefit while the spouse is alive
    spousal_start: NaiveDate,
    spousal: f64,
    // instead of the own benefit, if larger, after the spouse dies
    survivor_start: NaiveDate,
    survivor: f64,
}

#[derive(Debug, Clone, Default)]
pub struct HouseholdBenefits {
    retirees: Vec<RetireeBenefits>,
}

impl HouseholdBenefits {
    // one person, or a couple. With more than two, nobody gets spousal or
    // survivor benefits.
    pub fn new(people: &[Person]) -> Self {
        let retirees = people
            .iter()
            .enumerate()
            .map(|(i, person)| Self::benefits_for(person, spouse_of(people, i)))
            .collect();
        HouseholdBenefits { retirees }
    }

    fn benefits_for(person: &Person, spouse: Option<&Person>) -> RetireeBenefits {
        let dob = &person.date_of_birth;
        let claiming_age = person.claiming_age * 12;
        let own_start = add_months(dob, claiming_age);
        let own = person.pia * retirement_benefit_factor(dob, claiming_age);
        let mut benefits = RetireeBenefits {
            death_date: person.death_date,
            own_start,
            own,
            spousal_start: NaiveDate::MAX,
            spousal: 0.0,
            survivor_start: NaiveDate::MAX,
            survivor: 0.0,
        };
        let Some(spouse) = spouse else {
            return benefits;
        };

        // spousal: the part of half the spouse's PIA above this person's own
        // PIA, once both have claimed. Under deemed filing, claiming one
        // benefit claims both.
        let excess = spouse.pia / 2.0 - person.pia;
        if excess > 0.0 {
            let spouse_claims = add_months(&spouse.date_of_birth, spouse.claiming_age * 12);
            let start = own_start.max(spouse_claims);
            benefits.spousal_start = start;
            benefits.spousal = excess * spousal_benefit_factor(dob, age_in_months(dob, &start));
        }

        // survivor: starts when the spouse dies, but not before the earlier
        // of this person's own claiming age and their survivor FRA (waiting
        // past survivor FRA doesn't increase it)
        let survivor_fra_date = add_months(dob, survivor_full_retirement_age(dob));
        let start = spouse.death_date.max(own_start.min(survivor_fra_date));
        let factor = survivor_benefit_factor(dob, age_in_months(dob, &start));
        benefits.survivor_start = start;
        benefits.survivor = survivor_benefit(spouse, factor);
        benefits
    }

    // total monthly benefits for the household for the month starting on
    // date. Only retirees alive on that date receive benefits.
    pub fn monthly_total(&self, date: &NaiveDate) -> f64 {
        let mut total = 0.0;
        for (i, retiree) in self.retirees.iter().enumerate() {
            if *date >= retiree.death_date {
                continue;
            }
            let mut benefit = if *date >= retiree.own_start {
                retiree.own
            } else {
                0.0
            };
            if let Some(spouse) = spouse_of(&self.retirees, i) {
                if *date < spouse.death_date {
                    if *date >= retiree.spousal_start {
                        benefit += retiree.spousal;
                    }
                } else if *date >= retiree.survivor_start {
                    benefit = benefit.max(retiree.survivor);
                }
            }
            total += benefit;
        }
        total
    }
}

fn spouse_of<T>(people: &[T], i: usize) -> Option<&T> {
    if people.len() == 2 {
        people.get(1 - i)
    } else {
        None
    }
}

// the survivor benefit from the deceased, given the survivor's reduction
// factor for starting early
fn survivor_benefit(deceased: &Person, survivor_factor: f64) -> f64 {
    let dob = &deceased.date_of_birth;
    let fra = full_retirement_age(dob);
    let claiming_age = deceased.claiming_age * 12;
    let claimed = add_months(dob, claiming_age) < deceased.death_date;

    if claimed && claiming_age < fra {
        // the deceased claimed early: the survivor benefit is figured from
        // the full PIA, but limited to the larger of what the deceased was
        // receiving and 82.5% of the PIA
        let receiving = deceased.pia * retirement_benefit_factor(dob, claiming_age);
        let limit = receiving.max(deceased.pia * SURVIVOR_MINIMUM_OF_PIA);
        (deceased.pia * survivor_factor).min(limit)
    } else {
        // what the deceased was receiving, including delayed credits; or if
        // they died before claiming, the delayed credits earned by then
        let credited_age = if claimed {
            claiming_age
        } else {
            age_in_months(dob, &deceased.death_date).max(fra)
        };
        deceased.pia * retirement_benefit_factor(dob, credited_age.max(fra)) * survivor_factor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 0.01,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn test_full_retirement_age() {
        let fra = |y, m, d| full_retirement_age(&date(y, m, d));
        assert_eq!(fra(1937, 6, 1), 65 * 12);
        assert_eq!(fra(1938, 6, 1), 65 * 12 + 2);
        assert_eq!(fra(1942, 6, 1), 65 * 12 + 10);
        assert_eq!(fra(1943, 6, 1), 66 * 12);
        assert_eq!(fra(1954, 6, 1), 66 * 12);
        assert_eq!(fra(1957, 6, 1), 66 * 12 + 6);
        assert_eq!(fra(1959, 6, 1), 66 * 12 + 10);
        assert_eq!(fra(1960, 6, 1), 67 * 12);
        // born January 1: counted as born the year before
        assert_eq!(fra(1960, 1, 1), 66 * 12 + 10);
        assert_eq!(fra(1960, 1, 2), 67 * 12);
    }

    #[test]
    fn test_survivor_full_retirement_age() {
        let fra = |y| survivor_full_retirement_age(&date(y, 6, 1));
        assert_eq!(fra(1939), 65 * 12);
        assert_eq!(fra(1940), 65 * 12 + 2);
        assert_eq!(fra(1945), 66 * 12);
        assert_eq!(fra(1956), 66 * 12);
        assert_eq!(fra(1957), 66 * 12 + 2);
        assert_eq!(fra(1961), 66 * 12 + 10);
        assert_eq!(fra(1962), 67 * 12);
    }

    #[test]
    fn test_retirement_benefit_by_claiming_age() {
        // FRA 67: 70% at 62, 124% at 70
        let dob = date(1960, 6, 1);
        let factor = |age: u32| retirement_benefit_factor(&dob, age * 12);
        assert_close(factor(62), 0.70);
        assert_close(factor(63), 0.75);
        assert_close(factor(64), 0.80);
        assert_close(factor(65), 1.0 - 0.2 / 1.5);
        assert_close(factor(66), 1.0 - 0.2 / 3.0);
        assert_close(factor(67), 1.0);
        assert_close(factor(68), 1.08);
        assert_close(factor(70), 1.24);
        // no credits after 70
        assert_close(factor(72), 1.24);
        assert_close(retirement_benefit_factor(&dob, 67 * 12 + 6), 1.04);
    }

    #[test]
    fn test_retirement_benefit_other_birth_years() {
        // FRA 66 and 6 months: 54 months early at 62
        assert_close(retirement_benefit_factor(&date(1957, 6, 1), 62 * 12), 0.725);
        // FRA 66: 75% at 62, 132% at 70
        let dob = date(1950, 6, 1);
        assert_close(retirement_benefit_factor(&dob, 62 * 12), 0.75);
        assert_close(retirement_benefit_factor(&dob, 70 * 12), 1.32);
        // smaller delayed credits for earlier birth years
        assert_close(delayed_credit_per_year(&date(1940, 6, 1)), 0.07);
        assert_close(delayed_credit_per_year(&date(1942, 6, 1)), 0.075);
        assert_close(delayed_credit_per_year(&date(1943, 6, 1)), 0.08);
    }

    #[test]
    fn test_age_in_months() {
        let dob = date(1960, 9, 30);
        assert_eq!(age_in_months(&dob, &date(2022, 9, 29)), 62 * 12 - 1);
        assert_eq!(age_in_months(&dob, &date(2022, 9, 30)), 62 * 12);
        assert_eq!(age_in_months(&dob, &date(2022, 10, 30)), 62 * 12 + 1);
        // birthdays at the end of a month
        let leap = date(1964, 2, 29);
        assert_eq!(age_in_months(&leap, &date(1965, 2, 28)), 12);
        assert_eq!(age_in_months(&dob, &dob), 0);
    }

    // worker and spouse born on the same day in 1962 (FRA 67, survivor FRA
    // 67). Life expectancies far in the future unless given.
    fn person(pia: f64, claiming_age: u32) -> Person {
        Person {
            date_of_birth: date(1962, 6, 15),
            pia,
            claiming_age,
            death_date: date(2100, 1, 1),
        }
    }

    fn dies_at(mut person: Person, age: u32) -> Person {
        person.death_date = add_months(&person.date_of_birth, age * 12);
        person
    }

    fn at_age(age: u32) -> NaiveDate {
        add_months(&date(1962, 6, 15), age * 12)
    }

    fn at_age_months(months: u32) -> NaiveDate {
        add_months(&date(1962, 6, 15), months)
    }

    fn household(people: &[Person]) -> HouseholdBenefits {
        HouseholdBenefits::new(people)
    }

    #[test]
    fn test_single_retiree() {
        let h = household(&[person(3000.0, 70)]);
        assert_eq!(h.monthly_total(&at_age(69)), 0.0);
        assert_close(h.monthly_total(&at_age(70)), 3720.0);
        // nothing after death
        let h = household(&[dies_at(person(3000.0, 62), 80)]);
        assert_close(h.monthly_total(&at_age(79)), 2100.0);
        assert_eq!(h.monthly_total(&at_age(80)), 0.0);
    }

    #[test]
    fn test_spousal_benefit_for_nonworking_spouse() {
        // half the worker's PIA at the spouse's FRA, whenever the worker
        // claims. The worker's delayed credits don't increase it.
        for worker_age in [67, 70] {
            let h = household(&[person(3000.0, worker_age), person(0.0, 67)]);
            let worker = 3000.0 * retirement_benefit_factor(&date(1962, 6, 15), worker_age * 12);
            assert_close(h.monthly_total(&at_age(70)), worker + 1500.0);
        }

        // 32.5% of the worker's PIA at 62
        let h = household(&[person(3000.0, 62), person(0.0, 62)]);
        assert_close(h.monthly_total(&at_age(62)), 2100.0 + 975.0);

        // no delayed credits for the spouse
        let h = household(&[person(3000.0, 67), person(0.0, 70)]);
        assert_close(h.monthly_total(&at_age(70)), 3000.0 + 1500.0);
    }

    #[test]
    fn test_spousal_benefit_waits_for_worker() {
        // the spouse claims at 62, but the worker only at 65: the spousal
        // benefit starts then, reduced for 24 months early (16.67%)
        let h = household(&[person(3000.0, 65), person(0.0, 62)]);
        assert_eq!(h.monthly_total(&at_age(64)), 0.0);
        let worker = 3000.0 * (1.0 - 0.2 / 1.5);
        assert_close(
            h.monthly_total(&at_age(65)),
            worker + 1500.0 * (1.0 - 1.0 / 6.0),
        );
    }

    #[test]
    fn test_spousal_benefit_with_own_record() {
        // own PIA 1000 plus the excess of half the worker's 3000 PIA
        let h = household(&[person(3000.0, 67), person(1000.0, 67)]);
        assert_close(h.monthly_total(&at_age(67)), 3000.0 + 1500.0);
        // claimed early: both parts reduced, with their own rates
        let h = household(&[person(3000.0, 67), person(1000.0, 62)]);
        assert_close(h.monthly_total(&at_age(62)), 700.0);
        // excess starts at 67, when the worker claims: no reduction
        assert_close(h.monthly_total(&at_age(67)), 3000.0 + 700.0 + 500.0);
        // own PIA above half the worker's: no spousal benefit
        let h = household(&[person(3000.0, 67), person(1600.0, 67)]);
        assert_close(h.monthly_total(&at_age(67)), 4600.0);
    }

    #[test]
    fn test_survivor_gets_deceased_delayed_benefit() {
        let worker = dies_at(person(3000.0, 70), 75);
        let h = household(&[worker, person(0.0, 67)]);
        assert_close(h.monthly_total(&at_age(74)), 3720.0 + 1500.0);
        // the spousal benefit stops; the survivor gets what the worker got
        assert_close(h.monthly_total(&at_age(75)), 3720.0);
    }

    #[test]
    fn test_survivor_minimum_when_deceased_claimed_early() {
        // the worker received 70% of PIA, but the survivor gets 82.5%
        let worker = dies_at(person(3000.0, 62), 75);
        let h = household(&[worker, person(0.0, 67)]);
        assert_close(h.monthly_total(&at_age(75)), 2475.0);
    }

    #[test]
    fn test_survivor_benefit_if_deceased_never_claimed() {
        // died at 65, before claiming at 70: the survivor gets the PIA
        let h = household(&[dies_at(person(3000.0, 70), 65), person(0.0, 67)]);
        assert_eq!(h.monthly_total(&at_age(66)), 0.0);
        assert_close(h.monthly_total(&at_age(67)), 3000.0);

        // died at 69: plus the 16% of delayed credits earned by then
        let h = household(&[dies_at(person(3000.0, 70), 69), person(0.0, 67)]);
        assert_close(h.monthly_total(&at_age(69)), 3480.0);
    }

    #[test]
    fn test_survivor_benefit_reduced_if_started_early() {
        // the survivor claims at 62, so the survivor benefit starts at 62
        // when the worker has already died: 60 of 84 months early
        let worker = dies_at(person(3000.0, 70), 61);
        let h = household(&[worker, person(0.0, 62)]);
        let factor = 1.0 - 0.285 * 60.0 / 84.0;
        assert_close(h.monthly_total(&at_age(62)), 3000.0 * factor);

        // started at 64, with the deceased claimed early: the reduced
        // amount is still above 82.5%, so the limit applies
        let worker = dies_at(person(3000.0, 62), 64);
        let h = household(&[worker, person(0.0, 62)]);
        assert_close(h.monthly_total(&at_age(64)), 2475.0);
        // the worker collected for one month. Started at 62 and 1 month, the
        // reduced amount is below the limit.
        let mut worker = person(3000.0, 62);
        let death = at_age_months(62 * 12 + 1);
        worker.death_date = death;
        let h = household(&[worker, person(0.0, 62)]);
        let factor = 1.0 - 0.285 * 59.0 / 84.0;
        assert_close(h.monthly_total(&death), 3000.0 * factor);
    }

    #[test]
    fn test_survivor_switches_to_larger_own_benefit() {
        // the survivor takes the survivor benefit at survivor FRA, then
        // their own benefit at 70 when it's larger
        let worker = dies_at(person(2000.0, 67), 63);
        let h = household(&[worker, person(1800.0, 70)]);
        assert_eq!(h.monthly_total(&at_age(66)), 0.0);
        assert_close(h.monthly_total(&at_age(67)), 2000.0);
        assert_close(h.monthly_total(&at_age(70)), 1800.0 * 1.24);
    }

    #[test]
    fn test_survivor_with_larger_own_benefit() {
        let worker = dies_at(person(1000.0, 67), 70);
        let h = household(&[worker, person(3000.0, 67)]);
        assert_close(h.monthly_total(&at_age(70)), 3000.0);
    }
}
