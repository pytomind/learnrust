// =============================================================================
// Snapshot Eras & Chronological Partition Timeline (2009 – Present)
// =============================================================================
use clap::ValueEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SnapshotEra {
    /// 🏛️ All-Time Journey (2009 – Present / All Available Partitions)
    All,
    /// ⛏️ Satoshi Era (2009 – 2011 for BTC, 2015 – 2016 for ETH)
    Satoshi,
    /// 🪙 Early Halvings & ETH Genesis (2012 – 2016)
    Early,
    /// 🚀 SegWit & DeFi Run (2017 – 2020)
    Modern,
    /// ⚡ Modern Taproot & Layer-2 (2021 – 2026)
    Present,
}

impl SnapshotEra {
    pub fn description(&self) -> &'static str {
        match self {
            SnapshotEra::All => "🏛️ All-Time Journey (2009 – Present)",
            SnapshotEra::Satoshi => "⛏️ Satoshi Era (2009 – 2011)",
            SnapshotEra::Early => "🪙 Early Halvings & ETH Genesis (2012 – 2016)",
            SnapshotEra::Modern => "🚀 SegWit & DeFi Run (2017 – 2020)",
            SnapshotEra::Present => "⚡ Modern Taproot & Layer-2 (2021 – 2026)",
        }
    }

    /// Return the list of monthly partitions ("YYYY-MM") corresponding to this era
    pub fn get_months(&self, chain: &str) -> Vec<String> {
        let is_eth = chain.eq_ignore_ascii_case("eth") || chain.eq_ignore_ascii_case("ethereum");
        let start_year = if is_eth { 2015 } else { 2009 };
        let start_month = if is_eth { 7 } else { 1 };
        let end_year = 2026;
        let end_month = 10;

        let (range_start_year, range_start_month, range_end_year, range_end_month) = match self {
            SnapshotEra::All => (start_year, start_month, end_year, end_month),
            SnapshotEra::Satoshi => (start_year, start_month, if is_eth { 2016 } else { 2011 }, 12),
            SnapshotEra::Early => (if is_eth { 2015 } else { 2012 }, if is_eth { 7 } else { 1 }, 2016, 12),
            SnapshotEra::Modern => (2017, 1, 2020, 12),
            SnapshotEra::Present => (2021, 1, end_year, end_month),
        };

        let mut months = Vec::new();
        for y in range_start_year..=range_end_year {
            let m_start = if y == range_start_year { range_start_month } else { 1 };
            let m_end = if y == range_end_year { range_end_month } else { 12 };
            for m in m_start..=m_end {
                months.push(format!("{}-{:02}", y, m));
            }
        }

        if months.is_empty() {
            vec![format!("{}-{:02}", end_year, end_month)]
        } else {
            months
        }
    }
}

