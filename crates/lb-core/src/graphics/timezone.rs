//! Time zone of a clock element: an IANA name (`Europe/Moscow`) or none
//! for the local time of the computer running the channel.

use chrono::{Local, NaiveDateTime, TimeZone};
use chrono_tz::Tz;

/// Zones offered in the editor: (IANA name, label). The editor still keeps
/// any other valid name read from the configuration.
pub const CLOCK_ZONES: &[(&str, &str)] = &[
    ("UTC", "UTC"),
    ("Europe/Kaliningrad", "Калининград"),
    ("Europe/Moscow", "Москва"),
    ("Europe/Samara", "Самара"),
    ("Asia/Yekaterinburg", "Екатеринбург"),
    ("Asia/Omsk", "Омск"),
    ("Asia/Novosibirsk", "Новосибирск"),
    ("Asia/Krasnoyarsk", "Красноярск"),
    ("Asia/Irkutsk", "Иркутск"),
    ("Asia/Yakutsk", "Якутск"),
    ("Asia/Vladivostok", "Владивосток"),
    ("Asia/Magadan", "Магадан"),
    ("Asia/Kamchatka", "Камчатка"),
    ("Europe/Minsk", "Минск"),
    ("Europe/Kyiv", "Киев"),
    ("Europe/Chisinau", "Кишинёв"),
    ("Asia/Tbilisi", "Тбилиси"),
    ("Asia/Yerevan", "Ереван"),
    ("Asia/Baku", "Баку"),
    ("Asia/Almaty", "Алматы"),
    ("Asia/Tashkent", "Ташкент"),
    ("Asia/Bishkek", "Бишкек"),
    ("Asia/Dushanbe", "Душанбе"),
    ("Europe/London", "Лондон"),
    ("Europe/Berlin", "Берлин"),
    ("Europe/Paris", "Париж"),
    ("Europe/Istanbul", "Стамбул"),
    ("Asia/Dubai", "Дубай"),
    ("Asia/Shanghai", "Пекин"),
    ("Asia/Tokyo", "Токио"),
    ("America/New_York", "Нью-Йорк"),
    ("America/Chicago", "Чикаго"),
    ("America/Los_Angeles", "Лос-Анджелес"),
];

/// Whether `name` is a known IANA zone.
pub fn valid_zone(name: &str) -> bool {
    name.parse::<Tz>().is_ok()
}

/// `now` (local wall time of the channel) as wall time in zone `tz`.
/// No zone or an unknown name keeps the local time.
pub fn clock_time(now: NaiveDateTime, tz: Option<&str>) -> NaiveDateTime {
    let Some(tz) = tz.and_then(|n| n.parse::<Tz>().ok()) else { return now };
    // a local time skipped by a DST change has no instant: keep it as is
    let Some(local) = Local.from_local_datetime(&now).earliest() else { return now };
    local.with_timezone(&tz).naive_local()
}

/// Current UTC offset of `name` as "+03:00" (empty for an unknown zone).
pub fn zone_offset(name: &str) -> String {
    let Ok(tz) = name.parse::<Tz>() else { return String::new() };
    let now = chrono::Utc::now().with_timezone(&tz);
    now.format("%:z").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_between_zones() {
        let now = chrono::Local::now().naive_local();
        assert_eq!(clock_time(now, None), now);
        assert_eq!(clock_time(now, Some("Nowhere/City")), now);
        let utc = clock_time(now, Some("UTC"));
        let diff = (utc - chrono::Utc::now().naive_utc()).num_seconds().abs();
        assert!(diff < 5, "{diff}");
        assert!(CLOCK_ZONES.iter().all(|(n, _)| valid_zone(n)));
        assert_eq!(zone_offset("UTC"), "+00:00");
    }
}
