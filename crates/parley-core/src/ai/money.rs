//! Amounts in dollars, the one rounding rule of docs/PUBLIK-INTEGRATION.md §8: two decimals from
//! $0.10 up, otherwise two significant figures, so a fraction of a cent is never shown as $0.00.

const MICROS_PER_CENT: u64 = 10_000;
const TEN_CENTS: u64 = 100_000;

/// Two decimals from $0.10 up (`$3.12`, `$0.10`), otherwise two significant figures (`$0.049`,
/// `$0.0058`, `$0.000017`); 0 is `$0.00`. Integer arithmetic, half up.
pub fn format_dollars(micros: i64) -> String {
    let sign = if micros < 0 { "-" } else { "" };
    format!("{sign}{}", format_unsigned(micros.unsigned_abs()))
}

fn format_unsigned(micros: u64) -> String {
    if micros == 0 {
        return "$0.00".to_string();
    }
    if micros < TEN_CENTS {
        // Keep two significant digits: drop all but the leading two, rounding half up.
        let digits = micros.ilog10() + 1;
        let scale = if digits > 2 { 10u64.pow(digits - 2) } else { 1 };
        let rounded = (micros + scale / 2) / scale * scale;
        // 99,500 micros and up round to ten cents, which the cents rule writes.
        if rounded < TEN_CENTS {
            let digits = rounded.ilog10() + 1;
            // Two significant figures need 8 − digits decimals; micros never need more than 6.
            let decimals = (8 - digits.min(8)).min(6) as usize;
            let fraction = format!("{rounded:06}");
            return format!("$0.{}", &fraction[..decimals]);
        }
        return "$0.10".to_string();
    }
    let cents = (micros + MICROS_PER_CENT / 2) / MICROS_PER_CENT;
    format!("${}.{:02}", cents / 100, cents % 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_documented_amounts() {
        assert_eq!(format_dollars(3_120_000), "$3.12");
        assert_eq!(format_dollars(100_000), "$0.10");
        assert_eq!(format_dollars(49_000), "$0.049");
        assert_eq!(format_dollars(5_800), "$0.0058");
        assert_eq!(format_dollars(17), "$0.000017");
        assert_eq!(format_dollars(0), "$0.00");
    }

    #[test]
    fn rounding_is_half_up_in_both_ranges() {
        assert_eq!(format_dollars(3_125_000), "$3.13");
        assert_eq!(format_dollars(3_124_999), "$3.12");
        assert_eq!(format_dollars(104_999), "$0.10");
        assert_eq!(format_dollars(105_000), "$0.11");
        assert_eq!(format_dollars(48_500), "$0.049");
        assert_eq!(format_dollars(48_499), "$0.048");
        assert_eq!(format_dollars(99_499), "$0.099");
        assert_eq!(format_dollars(99_500), "$0.10");
        assert_eq!(format_dollars(1_000), "$0.0010");
        assert_eq!(format_dollars(19), "$0.000019");
        assert_eq!(format_dollars(1), "$0.000001");
        assert_eq!(format_dollars(45_000), "$0.045");
        assert_eq!(format_dollars(20_000_000), "$20.00");
        assert_eq!(format_dollars(-17), "-$0.000017");
        assert_eq!(format_dollars(i64::MIN).chars().next(), Some('-'));
    }
}
