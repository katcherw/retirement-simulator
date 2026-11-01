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
* Benefits collected before FRA while still working are withheld by the
* earnings test, and recalculated at FRA to credit the months withheld.
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
// the earnings test: wages above these amounts withhold benefits before FRA.
// 2026 values. They rise with average wages, so they're treated as constant
// in today's dollars.
const ANNUAL_EXEMPT_AMOUNT: f64 = 24_480.0;
const FRA_YEAR_EXEMPT_AMOUNT: f64 = 65_160.0;

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
    // wages before retiring, for the earnings test
    pub annual_earnings: f64,
    pub retirement_date: NaiveDate,
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

// the benefits for one retiree, worked out in advance: the dates, earnings,
// and life expectancies are known, so they don't change during a simulation
#[derive(Debug, Clone)]
struct RetireeBenefits {
    death_date: NaiveDate,
    fra_date: NaiveDate,
    // own benefit, and from FRA, the amount recalculated for the months
    // withheld by the earnings test
    own_start: NaiveDate,
    own: f64,
    own_from_fra: f64,
    // added to the own benefit while the spouse is alive, also recalculated
    // at FRA
    spousal_start: NaiveDate,
    spousal: f64,
    spousal_from_fra: f64,
    // instead of the own benefit, if larger, after the spouse dies
    survivor_start: NaiveDate,
    survivor: f64,
    // fraction of benefits withheld each month by this retiree's earnings
    withholding: Withholding,
}

// fraction (0 to 1) of benefits withheld each month by the earnings test,
// from first_month on. Months are counted from year 0 (see month_index).
#[derive(Debug, Clone, Default)]
struct Withholding {
    first_month: i32,
    fractions: Vec<f64>,
}

impl Withholding {
    fn get(&self, month: i32) -> f64 {
        usize::try_from(month - self.first_month)
            .ok()
            .and_then(|i| self.fractions.get(i))
            .copied()
            .unwrap_or(0.0)
    }

    fn is_full(&self, month: i32) -> bool {
        self.get(month) >= 1.0 - 1e-9
    }

    // months in [from, to) when benefits were completely withheld
    fn full_months(&self, from: NaiveDate, to: NaiveDate) -> u32 {
        (month_index(&from)..month_index(&to))
            .filter(|&m| self.is_full(m))
            .count() as u32
    }
}

fn month_index(date: &NaiveDate) -> i32 {
    date.year() * 12 + date.month0() as i32
}

fn first_of_month(month: i32) -> NaiveDate {
    NaiveDate::from_ymd_opt(month.div_euclid(12), month.rem_euclid(12) as u32 + 1, 1)
        .unwrap_or(NaiveDate::MAX)
}

#[derive(Debug, Clone, Default)]
pub struct HouseholdBenefits {
    retirees: Vec<RetireeBenefits>,
}

impl HouseholdBenefits {
    // one person, or a couple. With more than two, nobody gets spousal or
    // survivor benefits.
    pub fn new(people: &[Person]) -> Self {
        let mut retirees: Vec<RetireeBenefits> = people
            .iter()
            .enumerate()
            .map(|(i, person)| Self::benefits_before_earnings_test(person, spouse_of(people, i)))
            .collect();

        // the earnings test is figured from the benefits before withholding
        let withholdings: Vec<Withholding> = (0..people.len())
            .map(|i| earnings_test(&people[i], i, &retirees))
            .collect();
        for (retiree, withholding) in retirees.iter_mut().zip(withholdings) {
            retiree.withholding = withholding;
        }

        // at FRA, benefits are recalculated as if claimed later by the number
        // of months they were withheld
        let credited_months: Vec<u32> = retirees
            .iter()
            .map(|r| r.withholding.full_months(r.own_start, r.fra_date))
            .collect();
        for (i, person) in people.iter().enumerate() {
            let dob = &person.date_of_birth;
            let claiming_age = person.claiming_age * 12 + credited_months[i];
            retirees[i].own_from_fra =
                person.pia * retirement_benefit_factor(dob, claiming_age.min(LATEST_CREDIT_AGE));

            if let Some(spouse) = spouse_of(people, i) {
                let j = 1 - i;
                let r = &retirees[i];
                if r.spousal > 0.0 {
                    let (start, fra) = (r.spousal_start, r.fra_date);
                    let withheld = (month_index(&start)..month_index(&fra))
                        .filter(|&m| r.withholding.is_full(m) || retirees[j].withholding.is_full(m))
                        .count() as u32;
                    let age = age_in_months(dob, &start) + withheld;
                    let excess = spouse.pia / 2.0 - person.pia;
                    retirees[i].spousal_from_fra = excess * spousal_benefit_factor(dob, age);
                }

                // the survivor benefit is figured from the deceased's
                // recalculated benefit
                let factor =
                    survivor_benefit_factor(dob, age_in_months(dob, &retirees[i].survivor_start));
                retirees[i].survivor = survivor_benefit(spouse, credited_months[j], factor);
            }
        }

        HouseholdBenefits { retirees }
    }

    fn benefits_before_earnings_test(person: &Person, spouse: Option<&Person>) -> RetireeBenefits {
        let dob = &person.date_of_birth;
        let claiming_age = person.claiming_age * 12;
        let own_start = add_months(dob, claiming_age);
        let own = person.pia * retirement_benefit_factor(dob, claiming_age);
        let mut benefits = RetireeBenefits {
            death_date: person.death_date,
            fra_date: add_months(dob, full_retirement_age(dob)),
            own_start,
            own,
            own_from_fra: own,
            spousal_start: NaiveDate::MAX,
            spousal: 0.0,
            spousal_from_fra: 0.0,
            survivor_start: NaiveDate::MAX,
            survivor: 0.0,
            withholding: Withholding::default(),
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
            benefits.spousal_from_fra = benefits.spousal;
        }

        // survivor: starts when the spouse dies, but not before the earlier
        // of this person's own claiming age and their survivor FRA (waiting
        // past survivor FRA doesn't increase it)
        let survivor_fra_date = add_months(dob, survivor_full_retirement_age(dob));
        let start = spouse.death_date.max(own_start.min(survivor_fra_date));
        let factor = survivor_benefit_factor(dob, age_in_months(dob, &start));
        benefits.survivor_start = start;
        benefits.survivor = survivor_benefit(spouse, 0, factor);
        benefits
    }

    // total monthly benefits for the household for the month starting on
    // date. Only retirees alive on that date receive benefits.
    pub fn monthly_total(&self, date: &NaiveDate) -> f64 {
        let month = month_index(date);
        let mut total = 0.0;
        for (i, retiree) in self.retirees.iter().enumerate() {
            if *date >= retiree.death_date {
                continue;
            }
            let after_fra = *date >= retiree.fra_date;
            let own = if *date < retiree.own_start {
                0.0
            } else if after_fra {
                retiree.own_from_fra
            } else {
                retiree.own
            };
            let withheld = retiree.withholding.get(month);

            let benefit = match spouse_of(&self.retirees, i) {
                Some(spouse) if *date < spouse.death_date => {
                    let spousal = if *date < retiree.spousal_start {
                        0.0
                    } else if after_fra {
                        retiree.spousal_from_fra
                    } else {
                        retiree.spousal
                    };
                    // the spousal benefit is also withheld by the spouse's
                    // earnings, since it's paid on their record
                    let spouse_withheld = spouse.withholding.get(month);
                    own * (1.0 - withheld) + spousal * (1.0 - withheld) * (1.0 - spouse_withheld)
                }
                Some(_) if *date >= retiree.survivor_start => {
                    own.max(retiree.survivor) * (1.0 - withheld)
                }
                _ => own * (1.0 - withheld),
            };
            total += benefit;
        }
        total
    }
}

// benefits before withholding, in the given month, that retiree i's
// earnings can withhold: their own benefits, and the spousal benefit paid to
// their spouse on their record
fn benefits_subject_to_test(retirees: &[RetireeBenefits], i: usize, month: i32) -> f64 {
    let date = first_of_month(month);
    let entitled = |start: NaiveDate, amount: f64| {
        if month >= month_index(&start) {
            amount
        } else {
            0.0
        }
    };
    let retiree = &retirees[i];
    if date >= retiree.death_date {
        return 0.0;
    }
    let own = entitled(retiree.own_start, retiree.own);
    match spouse_of(retirees, i) {
        Some(spouse) if date < spouse.death_date => {
            own + entitled(retiree.spousal_start, retiree.spousal)
                + entitled(spouse.spousal_start, spouse.spousal)
        }
        Some(_) => own.max(entitled(retiree.survivor_start, retiree.survivor)),
        None => own,
    }
}

// the retirement earnings test, for a retiree who collects benefits before
// FRA while still working. For each year before the year they reach FRA,
// $1 is withheld for every $2 of wages above the annual exempt amount. In
// the year they reach FRA, $1 for every $3 above a higher amount, counting
// only wages before the FRA month. Benefits are withheld starting with the
// first month of the year. Months after retiring are never withheld (in the
// first year of retirement, a month without work is paid whatever the year's
// wages were).
fn earnings_test(person: &Person, i: usize, retirees: &[RetireeBenefits]) -> Withholding {
    let retiree = &retirees[i];
    let first_benefit = retiree.own_start.min(retiree.survivor_start);
    let fra_month = month_index(&retiree.fra_date);
    if person.annual_earnings <= 0.0 || first_benefit >= retiree.fra_date {
        return Withholding::default();
    }

    let first_month = month_index(&first_benefit).div_euclid(12) * 12;
    let mut fractions = vec![0.0; (fra_month - first_month).max(0) as usize];
    let works = |month: i32| {
        let date = first_of_month(month);
        date < person.retirement_date && date < retiree.death_date
    };

    for year_start in (first_month..fra_month).step_by(12) {
        let months = year_start..(year_start + 12).min(fra_month);
        let wages =
            months.clone().filter(|&m| works(m)).count() as f64 * person.annual_earnings / 12.0;
        let fra_year = year_start + 12 > fra_month;
        let mut to_withhold = if fra_year {
            (wages - FRA_YEAR_EXEMPT_AMOUNT).max(0.0) / 3.0
        } else {
            (wages - ANNUAL_EXEMPT_AMOUNT).max(0.0) / 2.0
        };

        for month in months {
            if to_withhold <= 0.0 {
                break;
            }
            let benefits = benefits_subject_to_test(retirees, i, month);
            if !works(month) || benefits <= 0.0 {
                continue;
            }
            let withheld = to_withhold.min(benefits);
            fractions[(month - first_month) as usize] = withheld / benefits;
            to_withhold -= withheld;
        }
    }

    Withholding {
        first_month,
        fractions,
    }
}

fn spouse_of<T>(people: &[T], i: usize) -> Option<&T> {
    if people.len() == 2 {
        people.get(1 - i)
    } else {
        None
    }
}

// the survivor benefit from the deceased, given the months the deceased's
// benefits were withheld by the earnings test (which count as if they had
// claimed later) and the survivor's reduction factor for starting early
fn survivor_benefit(deceased: &Person, withheld_months: u32, survivor_factor: f64) -> f64 {
    let dob = &deceased.date_of_birth;
    let fra = full_retirement_age(dob);
    let claimed = add_months(dob, deceased.claiming_age * 12) < deceased.death_date;
    let claiming_age = deceased.claiming_age * 12 + withheld_months;

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
            annual_earnings: 0.0,
            retirement_date: date(1962, 6, 15),
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

    // person born 1962-06-15 (FRA 67, in June 2029) who works until the
    // given age at the given salary
    fn working(mut person: Person, annual_earnings: f64, retirement_age: u32) -> Person {
        person.annual_earnings = annual_earnings;
        person.retirement_date = at_age(retirement_age);
        person
    }

    #[test]
    fn test_earnings_test_withholds_part_of_each_year() {
        // claims at 62 (June 2024, $2100) while earning $60,000 until 65
        // (June 2027). Each full year withholds (60,000 - 24,480) / 2 =
        // $17,760, from January on.
        let h = household(&[working(person(3000.0, 62), 60000.0, 65)]);
        // 2024: only June to December are benefit months, all withheld
        assert_eq!(h.monthly_total(&date(2024, 12, 15)), 0.0);
        // 2025: 8 months withheld, and $960 of September
        assert_eq!(h.monthly_total(&date(2025, 8, 15)), 0.0);
        assert_close(h.monthly_total(&date(2025, 9, 15)), 2100.0 - 960.0);
        assert_close(h.monthly_total(&date(2025, 10, 15)), 2100.0);
        // 2027: wages of $30,000 (January to June) withhold $2,760
        assert_eq!(h.monthly_total(&date(2027, 1, 15)), 0.0);
        assert_close(h.monthly_total(&date(2027, 2, 15)), 2100.0 - 660.0);
        assert_close(h.monthly_total(&date(2027, 7, 15)), 2100.0);
        assert_close(h.monthly_total(&date(2029, 5, 15)), 2100.0);
        // at FRA, recalculated for the 7 + 8 + 8 + 1 = 24 months withheld,
        // as if claimed at 64
        assert_close(h.monthly_total(&date(2029, 6, 15)), 2400.0);
    }

    #[test]
    fn test_earnings_test_high_salary_same_as_claiming_at_retirement() {
        // every month until retiring at 65 is withheld: June 2024 through
        // June 2027, 37 months
        let h = household(&[working(person(3000.0, 62), 200000.0, 65)]);
        assert_eq!(h.monthly_total(&date(2027, 6, 15)), 0.0);
        assert_close(h.monthly_total(&date(2027, 7, 15)), 2100.0);
        // as if claimed at 65 and 1 month: 23 months early
        let factor = 1.0 - 23.0 * 5.0 / 900.0;
        assert_close(h.monthly_total(&date(2029, 6, 15)), 3000.0 * factor);
    }

    #[test]
    fn test_earnings_test_in_fra_year() {
        // claims at 66 ($2800) and works past FRA at $180,000. 2028 is all
        // withheld. In 2029, only January to May wages count: (75,000 -
        // 65,160) / 3 = $3,280.
        let h = household(&[working(person(3000.0, 66), 180000.0, 68)]);
        assert_eq!(h.monthly_total(&date(2028, 12, 15)), 0.0);
        assert_eq!(h.monthly_total(&date(2029, 1, 15)), 0.0);
        assert_close(h.monthly_total(&date(2029, 2, 15)), 2800.0 - 480.0);
        assert_close(h.monthly_total(&date(2029, 3, 15)), 2800.0);
        // no test from FRA on, even while still working. Recalculated for
        // 8 months withheld: as if claimed at 66 and 8 months.
        let factor = 1.0 - 4.0 * 5.0 / 900.0;
        assert_close(h.monthly_total(&date(2029, 6, 15)), 3000.0 * factor);
    }

    #[test]
    fn test_no_earnings_test_after_retiring() {
        // retired at 62 before claiming: nothing withheld, even though the
        // year's wages are over the limit (months without work are paid)
        let h = household(&[working(person(3000.0, 62), 200000.0, 62)]);
        assert_close(h.monthly_total(&date(2024, 7, 15)), 2100.0);
    }

    #[test]
    fn test_earnings_test_withholds_spousal_benefit() {
        // the worker's wages also withhold the spousal benefit paid on
        // their record, and it's recalculated at FRA too
        let worker = working(person(3000.0, 62), 200000.0, 65);
        let h = household(&[worker, person(0.0, 62)]);
        assert_eq!(h.monthly_total(&date(2027, 6, 15)), 0.0);
        assert_close(h.monthly_total(&date(2027, 7, 15)), 2100.0 + 975.0);
        let own = 3000.0 * (1.0 - 23.0 * 5.0 / 900.0);
        let spousal = 1500.0 * (1.0 - 23.0 * 25.0 / 3600.0);
        assert_close(h.monthly_total(&date(2029, 6, 15)), own + spousal);
    }

    #[test]
    fn test_survivor_benefit_after_earnings_test() {
        // the deceased's benefit was recalculated as if claimed at 65 and 1
        // month (87.2% of PIA), above the 82.5% minimum
        let worker = dies_at(working(person(3000.0, 62), 200000.0, 65), 66);
        let h = household(&[worker, person(0.0, 67)]);
        let factor = 1.0 - 23.0 * 5.0 / 900.0;
        assert_close(h.monthly_total(&at_age(67)), 3000.0 * factor);
    }
}
