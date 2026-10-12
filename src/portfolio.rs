/**************************************************************************
* portfolio.rs
*
* Tracks the portfolio holdings and balance. Right now assume an asset
* allocation and continuous rebalancing. Every account holds the same
* allocation.
**************************************************************************/

use crate::utils::*;

// all values are percentages (0-100.0)
#[derive(Debug, Clone, Copy)]
pub struct Allocation {
    pub us_equities: f64,
    pub international: f64,
    pub bonds: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct Portfolio {
    // all accounts except Roth: 401(k), traditional IRA, brokerage, etc.
    // Withdrawals are taxed as ordinary income.
    pub pre_tax_balance: f64,
    // Roth accounts (and HSA). Withdrawals aren't taxed.
    pub roth_balance: f64,

    pub pre_retirement_allocation: Allocation,
    pub post_retirement_allocation: Allocation,

    pub us_equity_expected_returns: f64,
    pub us_equity_standard_deviation: f64,
    pub international_equity_expected_returns: f64,
    pub international_equity_standard_deviation: f64,
    pub bonds_expected_returns: f64,
    pub bonds_standard_deviation: f64,

    // correlation of annual returns between asset classes (-1.0 to 1.0),
    // used by the monte carlo simulation
    pub us_international_correlation: f64,
    pub us_bonds_correlation: f64,
    pub international_bonds_correlation: f64,
}

impl Portfolio {
    pub fn balance(&self) -> f64 {
        self.pre_tax_balance + self.roth_balance
    }

    // contributions and leftover income go to the pre-tax balance
    pub fn deposit(&mut self, amount: f64) {
        self.pre_tax_balance += amount;
    }

    // withdraws from the pre-tax balance first, then from Roth
    pub fn withdraw(&mut self, amount: f64) {
        let from_pre_tax = amount.min(self.pre_tax_balance);
        self.pre_tax_balance -= from_pre_tax;
        self.roth_balance = (self.roth_balance - (amount - from_pre_tax)).max(0.0);
    }

    // grows the balances and returns annualized average return
    pub fn grow(
        &mut self,
        us_equity_expected_returns: f64,
        international_equity_expected_returns: f64,
        bonds_expected_returns: f64,
        use_post_retirement: bool,
    ) -> f64 {
        let &allocation = if use_post_retirement {
            &self.post_retirement_allocation
        } else {
            &self.pre_retirement_allocation
        };
        let growth = (allocation.us_equities
            * (get_monthly_rate(us_equity_expected_returns / 100.0) + 1.0)
            + allocation.international
                * (get_monthly_rate(international_equity_expected_returns / 100.0) + 1.0)
            + allocation.bonds * (get_monthly_rate(bonds_expected_returns / 100.0) + 1.0))
            / 100.0;
        self.pre_tax_balance *= growth;
        self.roth_balance *= growth;

        // return annualized return
        us_equity_expected_returns * allocation.us_equities / 100.0
            + international_equity_expected_returns * allocation.international / 100.0
            + bonds_expected_returns * allocation.bonds / 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn portfolio(pre_tax_balance: f64, roth_balance: f64) -> Portfolio {
        let allocation = Allocation {
            us_equities: 50.0,
            international: 20.0,
            bonds: 30.0,
        };
        Portfolio {
            pre_tax_balance,
            roth_balance,
            pre_retirement_allocation: allocation,
            post_retirement_allocation: allocation,
            us_equity_expected_returns: 0.0,
            us_equity_standard_deviation: 0.0,
            international_equity_expected_returns: 0.0,
            international_equity_standard_deviation: 0.0,
            bonds_expected_returns: 0.0,
            bonds_standard_deviation: 0.0,
            us_international_correlation: 0.0,
            us_bonds_correlation: 0.0,
            international_bonds_correlation: 0.0,
        }
    }

    #[test]
    fn test_withdraw_pre_tax_first() {
        let mut p = portfolio(1000.0, 500.0);
        p.withdraw(800.0);
        assert_eq!((p.pre_tax_balance, p.roth_balance), (200.0, 500.0));
        p.withdraw(300.0);
        assert_eq!((p.pre_tax_balance, p.roth_balance), (0.0, 400.0));
        p.withdraw(1000.0);
        assert_eq!((p.pre_tax_balance, p.roth_balance), (0.0, 0.0));
    }

    #[test]
    fn test_deposit_to_pre_tax() {
        let mut p = portfolio(1000.0, 500.0);
        p.deposit(100.0);
        assert_eq!((p.pre_tax_balance, p.roth_balance), (1100.0, 500.0));
        assert_eq!(p.balance(), 1600.0);
    }

    #[test]
    fn test_both_accounts_grow_at_the_same_rate() {
        let mut p = portfolio(1000.0, 500.0);
        let annual = p.grow(10.0, 10.0, 10.0, true);
        assert!((annual - 10.0).abs() < 1e-9);
        let monthly = 1.1f64.powf(1.0 / 12.0);
        assert!((p.pre_tax_balance - 1000.0 * monthly).abs() < 1e-9);
        assert!((p.roth_balance - 500.0 * monthly).abs() < 1e-9);
    }
}
