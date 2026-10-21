//! Human friendly duration type used in configuration files.
//!
//! Accepted forms: a number of seconds (`90`, `2.5`), `"HH:MM:SS[.mmm]"`,
//! `"MM:SS"`, or unit strings like `"1h30m"`, `"90s"`, `"500ms"`.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

use crate::Ms;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Dur(pub Ms);

impl Dur {
    pub const ZERO: Dur = Dur(0);
    pub fn secs(s: i64) -> Self {
        Dur(s * 1000)
    }
    pub fn mins(m: i64) -> Self {
        Dur(m * 60_000)
    }
    pub fn ms(self) -> Ms {
        self.0
    }
    pub fn as_secs_f64(self) -> f64 {
        self.0 as f64 / 1000.0
    }
}

impl fmt::Display for Dur {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ms = self.0;
        let frac = ms.abs() % 1000;
        if frac == 0 { write!(f, "{}", crate::fmt_ms(ms)) } else { write!(f, "{}.{:03}", crate::fmt_ms(ms), frac) }
    }
}

impl FromStr for Dur {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.is_empty() {
            return Err("empty duration".into());
        }
        if let Ok(v) = s.parse::<f64>() {
            return Ok(Dur((v * 1000.0).round() as Ms));
        }
        if s.contains(':') {
            let parts: Vec<&str> = s.split(':').collect();
            if parts.len() > 3 {
                return Err(format!("bad duration '{s}'"));
            }
            let mut total = 0f64;
            for p in &parts {
                let v: f64 = p.parse().map_err(|_| format!("bad duration '{s}'"))?;
                total = total * 60.0 + v;
            }
            return Ok(Dur((total * 1000.0).round() as Ms));
        }
        // unit form: 1h30m15s500ms
        let mut total: f64 = 0.0;
        let mut num = String::new();
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c.is_ascii_digit() || c == '.' {
                num.push(c);
                i += 1;
                continue;
            }
            if c.is_whitespace() {
                i += 1;
                continue;
            }
            let v: f64 = num.parse().map_err(|_| format!("bad duration '{s}'"))?;
            num.clear();
            let (mult, adv) = match (c, chars.get(i + 1)) {
                ('m', Some('s')) => (1.0, 2),
                ('h', _) => (3_600_000.0, 1),
                ('m', _) => (60_000.0, 1),
                ('s', _) => (1000.0, 1),
                ('d', _) => (86_400_000.0, 1),
                _ => return Err(format!("bad duration unit in '{s}'")),
            };
            total += v * mult;
            i += adv;
        }
        if !num.is_empty() {
            return Err(format!("missing unit in '{s}'"));
        }
        Ok(Dur(total.round() as Ms))
    }
}

impl Serialize for Dur {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Dur {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            I(i64),
            F(f64),
            S(String),
        }
        match Raw::deserialize(d)? {
            Raw::I(v) => Ok(Dur(v * 1000)),
            Raw::F(v) => Ok(Dur((v * 1000.0).round() as Ms)),
            Raw::S(s) => s.parse().map_err(serde::de::Error::custom),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_forms() {
        assert_eq!("90".parse::<Dur>().unwrap(), Dur(90_000));
        assert_eq!("2.5".parse::<Dur>().unwrap(), Dur(2_500));
        assert_eq!("01:30:00".parse::<Dur>().unwrap(), Dur(5_400_000));
        assert_eq!("00:00:01.040".parse::<Dur>().unwrap(), Dur(1_040));
        assert_eq!("1:30".parse::<Dur>().unwrap(), Dur(90_000));
        assert_eq!("1h30m".parse::<Dur>().unwrap(), Dur(5_400_000));
        assert_eq!("500ms".parse::<Dur>().unwrap(), Dur(500));
        assert_eq!("2m 5s".parse::<Dur>().unwrap(), Dur(125_000));
        assert!("abc".parse::<Dur>().is_err());
        assert!("10x".parse::<Dur>().is_err());
    }

    #[test]
    fn roundtrip() {
        let d = Dur(3_723_040);
        assert_eq!(d.to_string(), "01:02:03.040");
        assert_eq!(d.to_string().parse::<Dur>().unwrap(), d);
    }
}
