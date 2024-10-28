/**************************************************************************
* simulate.rs
*
* Performs the simulation of a retirement scenario
**************************************************************************/

use crate::{Input, TaxLevel};
use chrono;
use chrono::NaiveDate;
use crate::utils::*;
use crate::portfolio::Portfolio;

// stores results of each month of the simulation
#[derive(Debug)]
pub struct MonthlySnapshot {
    pub date: NaiveDate,
    pub balance: f32,
    pub expenses: f32,
    pub income: f32,
    pub tax_rate: f32,
    pub taxes: f32,
    pub withdrawal_rate: f32,
    pub annualized_return: f32,
}
    
// values collected for each retiree during simulation to make
// reporting easier
#[derive(Debug)]
pub struct RetireeInfo {
    pub name: String,
    pub social_security_date: NaiveDate,
    pub date_of_birth: NaiveDate,
    social_security_income: f32,
}

#[derive(Debug, Default)]
pub struct SimulationResults {
    pub retirement_date: NaiveDate,
    pub retirement_age: u32,
    pub retirees: Vec<RetireeInfo>,
    pub monthly_snapshot: Vec<MonthlySnapshot>,
    pub average_return: f32,
}

fn is_everyone_dead(current_date: &NaiveDate, input: &Input) -> bool {
    for retiree in input.retirees.iter() {
        if get_age(&retiree.date_of_birth, &current_date) <= retiree.life_expectency {
            return false;
        }
    }
    return true;
}

fn get_taxes(mut monthly_income: f32, standard_deduction: f32, tax_rates: &Vec<TaxLevel>) -> (f32, f32) {
    let mut total_tax: f32 = 0.0;
    if monthly_income > standard_deduction / 12.0 {
        monthly_income -= standard_deduction / 12.0;
    }
    else {
        monthly_income = 0.0;
    }
        
    for tax_rate in tax_rates.iter() {
        if monthly_income * 12.0 <= tax_rate.income {
            return (total_tax + monthly_income * tax_rate.rate / 100.0, tax_rate.rate)
        }
        else {
            total_tax += tax_rate.income / 12.0 * tax_rate.rate / 100.0;
            monthly_income -= tax_rate.income / 12.0;
        }
    }
    panic!("Tax rate too high!");
}
    
// this is an estimate. The IRS has a big table for retirement income based on
// age and retirement date. This routine uses the values from the last row of
// the table, for younger retirees. The user will enter their personal values
// from the IRS web site and this routine will interpolate the rest. In the future
// the whole table should be entered.
fn get_social_security_monthly_income(
    retirement_age: u32,
    benefit_early: f32,
    benefit_full: f32,
    benefit_delayed: f32) -> f32 {

    let min_age = 62;
    let normal_age = 67;
    let max_age = 70;
    
    if retirement_age >= max_age {
        return benefit_delayed;
    }
    else if retirement_age < min_age {
        return 0.0;
    }
    else if retirement_age >= normal_age {
        return benefit_full +
            (benefit_delayed - benefit_full) *
            (retirement_age - normal_age) as f32/
            (max_age - normal_age) as f32;
    }
    else {
        return benefit_early +
            (benefit_full - benefit_early) *
            (retirement_age - min_age) as f32 /
            (normal_age - min_age) as f32;
    }
}

// represents a simulation run
pub struct Simulation<'a> {
    pub simulation_results_: SimulationResults,
   
    input_: &'a Input,
    current_date_: NaiveDate,
    portfolio_: Portfolio,
    expenses_: f32,
    tax_rates_: Vec<TaxLevel>,
    sum_of_returns_: f32,
}
    
impl<'a> Simulation<'a> {
    pub fn new(input: &'a Input) -> Self {
        let retirement_date = add_years(&input.retirees[0].date_of_birth, input.retirees[0].retirement_age);
        let current_date: NaiveDate = chrono::Utc::now().naive_utc().date();

        let mut simulation_results = SimulationResults {
            retirement_date,
            retirement_age: input.retirees[0].retirement_age,
            retirees: Vec::new(),
            monthly_snapshot: Vec::new(),
            average_return: 0.0,
        };
        
        for retiree in input.retirees.iter() {
            let retiree_result = RetireeInfo {
                name: retiree.name.to_string(),
                social_security_date: add_years(&retiree.date_of_birth, retiree.social_security_age),
                date_of_birth: retiree.date_of_birth.clone(),
                social_security_income: get_social_security_monthly_income(
                    retiree.social_security_age,
                    retiree.social_security_amount_early,
                    retiree.social_security_amount_full,
                    retiree.social_security_amount_delayed),
            };
            simulation_results.retirees.push(retiree_result);
        }

        let portfolio = input.portfolio.clone();
        let expenses = input.expenses.monthly;
        let tax_rates = input.tax_rates.tax_levels.to_vec();

        Self {
            simulation_results_: simulation_results,
            input_: input,
            current_date_: current_date,
            portfolio_: portfolio,
            expenses_: expenses,
            tax_rates_: tax_rates,
            sum_of_returns_: 0.0,
        }
    }

    // returns true if simulation finished
    pub fn run_simulation_one_month(
        &mut self,
        us_equity_expected_returns: f32,
        international_equity_expected_returns: f32,
        bonds_expected_returns: f32) -> Result<bool, String> {
        
        if is_everyone_dead(&self.current_date_, &self.input_) {
            return Ok(true);
        }
        
        // pre-retirement contributions
        for retiree in self.input_.retirees.iter() {
            if self.current_date_ < self.simulation_results_.retirement_date {
                let contribution = retiree.salary_annual * retiree.retirement_contribution_percent / 100.0;
                self.portfolio_.deposit(contribution / 12.0);
            }
        }

        // social security: before or after retirement
        let mut income = 0.0;
        for (i, _retiree) in self.input_.retirees.iter().enumerate() {
            if self.current_date_ > self.simulation_results_.retirees[i].social_security_date {
                income += self.simulation_results_.retirees[i].social_security_income;
            }
        }

        // social security is usually 85% taxable (ignore lower incomes)
        let mut taxable_income = income * 0.85;
        
        // pension income, before or after retirement
        for retiree in self.input_.retirees.iter() {
            let pension_date = add_years(&retiree.date_of_birth, retiree.pension_age);
            if self.current_date_ >= pension_date {
                income += retiree.pension_monthly_income;
                taxable_income += retiree.pension_monthly_income;
            }
        }

        // other retirement income
        for retiree in self.input_.retirees.iter() {
            if self.current_date_ >= self.simulation_results_.retirement_date {
                income += retiree.other_monthly_retirement_income;
                taxable_income += retiree.other_monthly_retirement_income;
            }
        }

        // required withdrawals, only after retirement
        let mut withdrawals = 0.0;
        if self.current_date_ >= self.simulation_results_.retirement_date {
            if income < self.expenses_ {
                withdrawals = self.expenses_ - income;
            }
        }

        // tax on income and withdrawals. tax rate on ss will be higher, but ignore that for now
        let (mut taxes, tax_rate) = get_taxes(
            withdrawals + taxable_income,
            self.input_.tax_rates.standard_deduction,
            &self.tax_rates_);

        // we need to withdraw more cash to cover taxes. But these withdrawals
        // will cost more taxes, causing more withdrawals, and more taxes and so
        // on. This can be calculated as an infinite power series.
        let tax_on_tax = taxes / (1.0 - tax_rate / 100.0);
        taxes = tax_on_tax;
        
        let mut withdrawal_rate = 0.0;
        if self.portfolio_.balance > 0.0 {
            withdrawal_rate = (withdrawals + taxes) * 12.0 / self.portfolio_.balance;
        }
            
        if income > self.expenses_ {
            self.portfolio_.deposit(income - self.expenses_);
        }
        if self.portfolio_.balance > taxes {
            self.portfolio_.withdraw(taxes);
        }
        else {
            self.portfolio_.balance = 0.0
        }
        if self.portfolio_.balance > withdrawals {
            self.portfolio_.withdraw(withdrawals);
        }
        else {
            self.portfolio_.balance = 0.0
        }

        let annualized_return = self.portfolio_.grow(
            us_equity_expected_returns,
            international_equity_expected_returns,
            bonds_expected_returns,
            self.current_date_ >= self.simulation_results_.retirement_date);
        self.sum_of_returns_ += annualized_return;
        self.simulation_results_.average_return = self.sum_of_returns_ / (self.simulation_results_.monthly_snapshot.len() as f32 + 1.0); 

        let monthly_balance = MonthlySnapshot {
            date: self.current_date_,
            balance: self.portfolio_.balance,
            expenses: if self.current_date_ >= self.simulation_results_.retirement_date {self.expenses_} else {0.0}, 
            income,
            taxes,
            tax_rate,
            withdrawal_rate,
            annualized_return,
        };

        self.simulation_results_.monthly_snapshot.push(monthly_balance);


        self.current_date_ = self.current_date_.checked_add_months(chrono::Months::new(1)).unwrap();

        Ok(self.portfolio_.balance == 0.0)
    }
}        
    
pub fn run_simulation(input: &Input) -> Result<SimulationResults, String> {
    let mut simulation = Simulation::new(input);

    loop {
        let is_finished = simulation.run_simulation_one_month(
            input.portfolio.us_equity_expected_returns,
            input.portfolio.international_equity_expected_returns,
            input.portfolio.bonds_expected_returns)?;

        if is_finished {
            break;
        }
    }

    Ok(simulation.simulation_results_)
}
    
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_social_security() {
        let early = 1000.0;
        let full = 2000.0;
        let delayed = 4000.0;

        assert_eq!(get_social_security_monthly_income(60, early, full, delayed), 0.0);
        assert_eq!(get_social_security_monthly_income(62, early, full, delayed), early);
        assert_eq!(get_social_security_monthly_income(67, early, full, delayed), full);
        assert_eq!(get_social_security_monthly_income(70, early, full, delayed), delayed);
        assert_eq!(get_social_security_monthly_income(71, early, full, delayed), delayed);
        assert_eq!(get_social_security_monthly_income(63, early, full, delayed), 1200.0);
        assert_eq!(get_social_security_monthly_income(68, early, full, delayed), 2000.0 + 2000.0/3.0);
    }
}

        
