use crate::utils::*;

#[derive(Debug, Clone, Copy)]
pub struct Portfolio {
    pub balance: f32,
    
    pub us_equity_allocation: f32,
    pub international_equity_allocation: f32,
    pub bond_allocation: f32,
    
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
    
    pub fn grow(&mut self) {
        let mut us_equity = self.balance * self.us_equity_allocation / 100.0;
        let mut international_equity = self.balance * self.international_equity_allocation / 100.0;
        let mut bonds = self.balance * self.bond_allocation / 100.0;

        us_equity *= get_monthly_rate(self.us_equity_expected_returns / 100.0) + 1.0;
        international_equity *= get_monthly_rate(self.international_equity_expected_returns / 100.0) + 1.0;
        bonds *= get_monthly_rate(self.bonds_expected_returns / 100.0) + 1.0;

        self.balance = us_equity + international_equity + bonds;
        println!("balance = {}", self.balance);
    }
}

