/**************************************************************************
* portfolio.rs
*
* Tracks the portfolio holdings and balance. Right now assume an asset
* allocation and continuous rebalancing.
**************************************************************************/

use crate::utils::*;

// all values are percentages (0-100.0)
#[derive(Debug, Clone, Copy)]
pub struct Allocation {
    pub us_equities: f32,
    pub international: f32,
    pub bonds: f32,
}

impl Allocation {
    pub fn new() -> Self {
        Allocation {
            us_equities: 0.0,
            international: 0.0,
            bonds: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Portfolio {
    pub balance: f32,
    
    pub pre_retirement_allocation: Allocation,
    pub post_retirement_allocation: Allocation,
    
    pub us_equity_expected_returns: f32,
    pub us_equity_standard_deviation: f32,
    pub international_equity_expected_returns: f32,
    pub international_equity_standard_deviation: f32,
    pub bonds_expected_returns: f32,
    pub bonds_standard_deviation: f32,
    pub expected_inflation: f32,
}

impl Portfolio {
    pub fn deposit(&mut self, amount: f32) {
        self.balance += amount;
    }

    pub fn withdraw(&mut self, amount: f32) {
        self.balance -= amount;
        if self.balance < 0.0 {
            self.balance = 0.0;
        }
    }
    
    // grows the balance and returns annualized average return
    pub fn grow(
        &mut self,
        us_equity_expected_returns: f32,
        international_equity_expected_returns: f32,
        bonds_expected_returns: f32,
        use_post_retirement: bool) -> f32 {
        let &allocation = if use_post_retirement {&self.pre_retirement_allocation}
            else {&self.post_retirement_allocation};
        let mut us_equity = self.balance * allocation.us_equities / 100.0;
        let mut international_equity = self.balance * allocation.international / 100.0;
        let mut bonds = self.balance * allocation.bonds / 100.0;

        us_equity *= get_monthly_rate(us_equity_expected_returns / 100.0) + 1.0;
        international_equity *= get_monthly_rate(international_equity_expected_returns / 100.0) + 1.0;
        bonds *= get_monthly_rate(bonds_expected_returns / 100.0) + 1.0;

        self.balance = us_equity + international_equity + bonds;

        // return annualized return
        us_equity_expected_returns * allocation.us_equities / 100.0 +
            international_equity_expected_returns * allocation.international / 100.0 +
            bonds_expected_returns * allocation.bonds / 100.0
    }
}

