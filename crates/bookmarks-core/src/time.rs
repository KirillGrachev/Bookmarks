//! Работа со временем.
//!
//! Все метки времени в Bookmarks — целые миллисекунды с эпохи Unix в UTC.
//! Такой формат одинаково дёшев для SQLite (`INTEGER`), для IPC (число, а не
//! строка, которую нужно парсить) и для фронтенда (`Intl.DateTimeFormat` с
//! локалью пользователя). Часовой пояс хранится отдельно в настройках и
//! влияет только на отображение.

use chrono::{DateTime, TimeZone, Utc};

/// Метка времени: миллисекунды с эпохи Unix в UTC.
/// `Default` даёт начало эпохи (0) — удобная «пустая» метка для отчётов,
/// где поле обязательно, но значение ещё не известно.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(transparent)]
pub struct Timestamp(pub i64);

impl Timestamp {
    /// Текущий момент.
    pub fn now() -> Self {
        Self(Utc::now().timestamp_millis())
    }

    /// Метка из числа миллисекунд.
    pub const fn from_millis(millis: i64) -> Self {
        Self(millis)
    }

    /// Значение в миллисекундах.
    pub const fn millis(self) -> i64 {
        self.0
    }

    /// Преобразование в `chrono::DateTime<Utc>` (для арифметики и форматирования в Rust).
    pub fn to_datetime(self) -> Option<DateTime<Utc>> {
        Utc.timestamp_millis_opt(self.0).single()
    }

    /// Метка, сдвинутая на `days` суток вперёд (отрицательное значение — назад).
    pub fn checked_add_days(self, days: i64) -> Option<Self> {
        let datetime = self.to_datetime()?;
        let shifted = datetime.checked_add_signed(chrono::Duration::days(days))?;
        Some(Self(shifted.timestamp_millis()))
    }
}

impl std::fmt::Display for Timestamp {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl From<i64> for Timestamp {
    fn from(value: i64) -> Self {
        Self(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_is_sane_and_monotonic() {
        let first = Timestamp::now();
        let second = Timestamp::now();
        // 2020-01-01 .. 2100-01-01 — грубая, но достаточная проверка единиц измерения.
        assert!(first.millis() > 1_577_836_800_000);
        assert!(first.millis() < 4_102_444_800_000);
        assert!(second >= first);
    }

    #[test]
    fn roundtrips_through_datetime() {
        let timestamp = Timestamp::from_millis(1_700_000_000_123);
        let datetime = timestamp.to_datetime().expect("valid timestamp");
        assert_eq!(datetime.timestamp_millis(), timestamp.millis());
        assert!(Timestamp::from_millis(i64::MAX).to_datetime().is_none());
    }

    #[test]
    fn adds_days() {
        let timestamp = Timestamp::from_millis(0);
        assert_eq!(
            timestamp.checked_add_days(1).map(|value| value.millis()),
            Some(86_400_000)
        );
        assert_eq!(
            timestamp.checked_add_days(-1).map(|value| value.millis()),
            Some(-86_400_000)
        );
    }
}
