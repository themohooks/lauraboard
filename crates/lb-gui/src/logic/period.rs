//! Report periods: presets and validation of typed dates.

use chrono::{Duration, NaiveDate};

/// Longest period a report may cover.
pub const MAX_DAYS: i64 = 366;

/// Preset periods ending today (or yesterday).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Today,
    Yesterday,
    Week,
    Month,
}

impl Preset {
    pub const ALL: [Preset; 4] = [Preset::Today, Preset::Yesterday, Preset::Week, Preset::Month];

    pub fn range(self, today: NaiveDate) -> (NaiveDate, NaiveDate) {
        match self {
            Preset::Today => (today, today),
            Preset::Yesterday => (today - Duration::days(1), today - Duration::days(1)),
            Preset::Week => (today - Duration::days(6), today),
            Preset::Month => (today - Duration::days(29), today),
        }
    }

    /// Which preset (if any) a range corresponds to.
    pub fn of(from: NaiveDate, to: NaiveDate, today: NaiveDate) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.range(today) == (from, to))
    }
}

/// Field errors of a typed period (`None` = fine).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PeriodErrors {
    pub from: Option<String>,
    pub to: Option<String>,
}

fn parse(s: &str) -> Result<NaiveDate, String> {
    let s = s.trim();
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(s, "%d.%m.%Y"))
        .map_err(|_| "Дата в виде ГГГГ-ММ-ДД или ДД.ММ.ГГГГ".to_string())
}

/// Validate a period: both dates parse, `from <= to`, not in the future,
/// at most [`MAX_DAYS`] long.
pub fn validate(from: &str, to: &str, today: NaiveDate) -> Result<(NaiveDate, NaiveDate), PeriodErrors> {
    let (f, t) = (parse(from), parse(to));
    let mut e = PeriodErrors { from: f.as_ref().err().cloned(), to: t.as_ref().err().cloned() };
    let (Ok(f), Ok(t)) = (f, t) else { return Err(e) };
    if t > today {
        e.to = Some("Дата в будущем".into());
    }
    if f > t {
        e.from = Some("Начало позже конца".into());
    } else if (t - f).num_days() >= MAX_DAYS {
        e.from = Some(format!("Не больше {MAX_DAYS} дней"));
    }
    if e.from.is_some() || e.to.is_some() { Err(e) } else { Ok((f, t)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn presets_and_detection() {
        let today = d("2026-10-21");
        assert_eq!(Preset::Week.range(today), (d("2026-10-15"), today));
        assert_eq!(Preset::Yesterday.range(today), (d("2026-10-20"), d("2026-10-20")));
        assert_eq!(Preset::of(d("2026-09-22"), today, today), Some(Preset::Month));
        assert_eq!(Preset::of(d("2026-09-23"), today, today), None);
    }

    #[test]
    fn validation() {
        let today = d("2026-10-21");
        assert_eq!(validate("2026-10-01", "21.10.2026", today), Ok((d("2026-10-01"), today)));
        let e = validate("2026-10-22", "2026-10-21", today).unwrap_err();
        assert!(e.from.is_some() && e.to.is_none());
        let e = validate("вчера", "2026-10-30", today).unwrap_err();
        assert!(e.from.is_some() && e.to.as_deref() == Some("Дата в будущем"));
        assert!(validate("2025-01-01", "2026-10-21", today).unwrap_err().from.unwrap().contains("366"));
    }
}
