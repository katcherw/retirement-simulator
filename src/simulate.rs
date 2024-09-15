use crate::{Input, TaxLevel};
use chrono;
use chrono::{Date, NaiveDate, Local, Utc, Duration};

#[derive(Debug)]
pub struct MonthlySnapshot {
    pub date: NaiveDate,
    pub balance: u32,
    pub expenses: u32,
    pub income: u32,
    pub tax_rate: f32,
    pub taxes: u32,
    pub withdrawal_rate: f32,
}
    
#[derive(Debug)]
pub struct RetireeInfo {
    pub name: String,
    pub social_security_date: NaiveDate,
    social_security_income: u32,
}

#[derive(Debug)]
pub struct SimulationResults {
    pub retirement_date: NaiveDate,
    pub retirees: Vec<RetireeInfo>,
    pub monthly_snapshot: Vec<MonthlySnapshot>,
}

fn get_age(date_of_birth: &NaiveDate, current_date: &NaiveDate) -> u32 {
    let years_diff = current_date.years_since(*date_of_birth);
    match years_diff {
        Some(v) => v,
        None => 0,
    }
}

fn is_everyone_dead(current_date: &NaiveDate, input: &Input) -> bool {
    for (i, retiree) in input.retirees.iter().enumerate() {
        if get_age(&retiree.date_of_birth, &current_date) <= retiree.life_expectency {
            return false;
        }
    }
    return true;
}

fn add_years(date: &NaiveDate, years: u32) -> NaiveDate {
    let days_to_add = years as i64 * 365;
    match date.checked_add_signed(Duration::days(days_to_add)) {
        Some(v) => v,
        None => *date,
    }
}

fn get_monthly_rate(annual_rate: f32) -> f32 {
    // growth rates are expressed as rates compunded annually, but we will
    // calculate on a monthly basis
    (1.0 + annual_rate).powf(1.0 / 12.0) - 1.0
}

fn get_taxes(mut monthly_income: u32, standard_deduction: u32, tax_rates: &Vec<TaxLevel>) -> (u32, f32) {
    let mut total_tax: u32 = 0;
    if monthly_income > standard_deduction / 12 {
        monthly_income -= standard_deduction / 12;
    }
    else {
        monthly_income = 0;
    }
        
    for tax_rate in tax_rates.iter() {
        if monthly_income * 12 <= tax_rate.income {
            return (total_tax + (monthly_income as f32 * tax_rate.rate / 100.0) as u32, tax_rate.rate)
        }
        else {
            total_tax += (tax_rate.income as f32 / 12.0 * tax_rate.rate / 100.0) as u32;
            monthly_income -= (tax_rate.income as f32 / 12.0) as u32;
        }
    }
    panic!("Tax rate too high!");
}
    
fn get_social_security_monthly_income(
    retirement_age: u32,
    benefit_early: u32,
    benefit_full: u32,
    benefit_delayed: u32) -> u32 {

    let min_age = 62;
    let normal_age = 67;
    let max_age = 70;
    
    if retirement_age >= max_age {
        return benefit_delayed;
    }
    else if retirement_age < min_age {
        return 0;
    }
    else if retirement_age >= normal_age {
        return benefit_full +
            (benefit_delayed - benefit_full) *
            (retirement_age - normal_age) /
            (max_age - normal_age);
    }
    else {
        return benefit_early +
            (benefit_full - benefit_early) *
            (retirement_age - min_age) /
            (normal_age - min_age);
    }
}

pub fn run_simulation(input: &Input) -> Result<SimulationResults, String> {
    println!("starting simulation");

    let retirement_date = add_years(&input.retirees[0].date_of_birth, input.retirees[0].retirement_age);
    let mut simulation_results = SimulationResults {
        retirement_date,
        retirees: Vec::new(),
        monthly_snapshot: Vec::new(),
    };

    let mut current_date: NaiveDate = chrono::Utc::now().naive_utc().date();
    println!("now = {:?}", current_date);

    for retiree in input.retirees.iter() {
        let retiree_result = RetireeInfo {
            name: retiree.name.to_string(),
            social_security_date: add_years(&retiree.date_of_birth, retiree.social_security_age),
            social_security_income: get_social_security_monthly_income(
                retiree.social_security_age,
                retiree.social_security_amount_early,
                retiree.social_security_amount_full,
                retiree.social_security_amount_delayed),
        };
        simulation_results.retirees.push(retiree_result);
    }
            
    let years_diff = current_date.years_since(input.retirees[0].date_of_birth).ok_or("Invalid date")?;
    println!("years_diff = {}", years_diff);

    let mut balance = input.portfolio.balance;
    let mut expenses = input.expenses.monthly;
    let mut tax_rates = input.tax_rates.tax_levels.to_vec();
    
    let monthly_return = get_monthly_rate(input.portfolio.annual_return_percent / 100.0);

    while !is_everyone_dead(&current_date, &input) {
        // pre-retirement contributions
        for (i, retiree) in input.retirees.iter().enumerate() {
            if current_date < retirement_date {
                let contribution = retiree.salary_annual as f32 * retiree.retirement_contribution_percent / 100.0;
                balance += (contribution / 12.0) as u32;
            }
        }

        // social security: before or after retirement
        let mut income = 0;
        for (i, retiree) in input.retirees.iter().enumerate() {
            if current_date > simulation_results.retirees[i].social_security_date {
                income += simulation_results.retirees[i].social_security_income;
            }
        }

        // social security is usually 85% taxable (ignore lower incomes)
        let taxable_income = (income as f32 * 0.85) as u32;
        
        // required withdrawals, only after retirement
        let mut withdrawals: u32 = 0;
        if current_date >= retirement_date {
            if income < expenses {
                withdrawals = expenses - income;
            }
        }

        // tax on income and withdrawals. tax rate on ss will be higher, but ignore that for now
        let mut taxes: u32 = 0;
        let mut tax_rate: f32 = 0.0;
        (taxes, tax_rate) = get_taxes(
            withdrawals + taxable_income,
            input.tax_rates.standard_deduction,
            &tax_rates);

        // we need to withdraw more cash to cover taxes. But these withdrawals
        // will cost more taxes, causing more withdrawals, and more taxes and so
        // on. This can be calculated as an infinite power series.
        let tax_on_tax = (taxes as f32 / (1.0 - tax_rate / 100.0)) as u32;
        println!("taxes = {} tax_rate = {} tax_on_tax = {}", taxes, tax_rate, tax_on_tax);
        taxes = tax_on_tax;
        
        let mut withdrawal_rate = 0.0;
        if balance > 0 {
            withdrawal_rate = ((withdrawals + taxes) * 12) as f32 / balance as f32;
        }
            
        if income > expenses {
            balance += income - expenses;
        }
        if balance > taxes {
            balance -= taxes;
        }
        else {
            balance = 0;
        }
        if balance > withdrawals {
            balance -= withdrawals;
        }
        else {
            balance = 0;
        }

        let monthly_balance = MonthlySnapshot {
            date: current_date,
            balance,
            expenses, 
            income,
            taxes,
            tax_rate,
            withdrawal_rate,
        };

        simulation_results.monthly_snapshot.push(monthly_balance);

        balance = ((balance as f32) * (monthly_return + 1.0)) as u32;

        current_date = match current_date.checked_add_months(chrono::Months::new(1)) {
            Some(v) => v,
            None => return Err("Can't increment current date".to_string()),
        };
    }
    
    Ok(simulation_results)
}
    
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_social_security() {
        let early = 1000u32;
        let full = 2000u32;
        let delayed = 4000u32;

        assert_eq!(get_social_security_monthly_income(60, early, full, delayed), 0);
        assert_eq!(get_social_security_monthly_income(62, early, full, delayed), early);
        assert_eq!(get_social_security_monthly_income(67, early, full, delayed), full);
        assert_eq!(get_social_security_monthly_income(70, early, full, delayed), delayed);
        assert_eq!(get_social_security_monthly_income(71, early, full, delayed), delayed);
        assert_eq!(get_social_security_monthly_income(63, early, full, delayed), 1200);
        assert_eq!(get_social_security_monthly_income(68, early, full, delayed), ((2000 as f32) + (2000.0/3.0)) as u32);
    }
}

        
