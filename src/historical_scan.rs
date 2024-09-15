use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;

#[derive(Debug, Default)]
struct HistoricalAnnualReturns {
    year: u32,
    inflation: f32,

    // all returns are real returns (after inflation)
    sp500return: f32,
    tbill3month: f32,
    tbill10year: f32,
    corpBonds: f32,
    realEstate: f32,
    international: Option<f32>,
}

struct HistoricalReturns {
    annual_returns: Vec<HistoricalAnnualReturns>,
    averages: HistoricalAnnualReturns,
}

fn str_to_u32(s: &str) -> Result<u32, String> {
    s.trim().parse::<u32>().map_err(|v| (format!("Invalid integer: {}", v)))
}

fn str_to_f32(s: &str) -> Result<f32, String> {
    s.trim().parse::<f32>().map_err(|v| (format!("Invalid floating point: {}", v)))
}

fn str_to_f32_optional(s: &str) -> Option<f32> {
    match s.trim().parse::<f32>() {
        Ok(v) => Some(v),
        Err(_) => None,
    }
}

fn calculate_average(vals: &[f32]) -> f32 {
    let mut total = 0f32;

    for val in vals {
        total += val;
    }

    if vals.len() > 0 { total / vals.len() as f32} else { 0.0 }
}

fn calculate_average_optional(vals: &[Option<f32>]) -> f32 {
    let mut total = 0f32;

    for val in vals {
        total += val.unwrap_or(0.0);
    }

    if vals.len() > 0 { total / vals.len() as f32} else { 0.0 }
}

fn calculate_averages(returns: &[HistoricalAnnualReturns]) -> HistoricalAnnualReturns {
    let mut totals = HistoricalAnnualReturns::default();
    totals.international = Some(0.0);
    let mut international_count = 0.0;
    
    for ret in returns.iter() {
        totals.inflation += ret.inflation;
        totals.sp500return += ret.sp500return;
        totals.tbill3month += ret.tbill3month;
        totals.tbill10year += ret.tbill10year;
        totals.corpBonds += ret.corpBonds;
        totals.realEstate += ret.realEstate;
        if ret.international.is_some() {
            totals.international = Some(totals.international.unwrap() +
                ret.international.unwrap());
            international_count += 1.0;
        }
    };

    let mut averages = HistoricalAnnualReturns::default();
    let count = returns.len() as f32;
    if count > 0.0 {
        averages.inflation = totals.inflation / count;
        averages.sp500return = totals.sp500return / count;
        averages.tbill3month = totals.tbill3month / count;
        averages.tbill10year = totals.tbill10year / count;
        averages.corpBonds = totals.corpBonds / count;
        averages.realEstate = totals.realEstate / count;
        averages.international = Some(if international_count > 0.0
            { totals.international.unwrap() / international_count } else { 0.0 });
    };

    averages    
}

fn parse_returns() -> Result<HistoricalReturns, String> {

    let mut annual_returns: Vec<HistoricalAnnualReturns> = Vec::new();

    let fname = Path::new("returns.csv");
    let file = File::open(fname).map_err(|_| "Can't open returns.csv")?;
    let reader = io::BufReader::new(file);

    for (i, line) in reader.lines().enumerate() {
        if i < 2 {
            continue;
        }
        let line = line.map_err(|v| format!("Can't read line from returns.csv: {}", v.to_string()))?;
        let toks: Vec<&str> = line.split(',').collect();
        if toks.len() < 14 {
            return Err(format!("Can't parse line [{}]", line));
        }

        let year = str_to_u32(toks[0])?;
        let inflation = str_to_f32(toks[8])?;
        let sp500return = str_to_f32(toks[9])?;
        let tbill3month = str_to_f32(toks[10])?;
        let tbill10year = str_to_f32(toks[11])?;
        let corpBonds = str_to_f32(toks[12])?;
        let realEstate = str_to_f32(toks[13])?;
        let international = str_to_f32_optional(toks[14]);

        let returns = HistoricalAnnualReturns {
            year,
            inflation,
            sp500return,
            tbill3month,
            tbill10year,
            corpBonds,
            realEstate,
            international,
        };

        //println!("{:?}", returns);

        annual_returns.push(returns);
    }

    let averages = calculate_averages(&annual_returns);
    let historical_returns = HistoricalReturns {
        annual_returns,
        averages
    };
    Ok(historical_returns)
}
    
pub fn run_historical_scan() {
    let returns = parse_returns();
    match parse_returns() {
        Ok(v) => println!("Averages: {:?}", v.averages),
        Err(e) => println!("{}", e),
    };
    
}
