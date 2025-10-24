#[cfg(test)]
mod tests {
    use crate::tests::test::{get_report, report_has_error_with_str_includes};

    #[test]
    fn test_single_matcher_matches_good_regex() {
        assert!(
            get_report("test", "`id:/test/`").is_valid(),
            "Expected match to be valid"
        );
    }

    #[test]
    fn test_single_matcher_matches_bad_regex() {
        let report = get_report("testttt", "`id:/test/`");
        print!("{:?}", report);
        assert!(
            !report.is_valid(),
            "Expected match to be invalid due to regex mismatch"
        );
        assert!(
            report_has_error_with_str_includes(
                &report,
                "Matcher mismatch: input 'testttt' does not conform to #id:/test/"
            ),
            "Expected error message for regex mismatch"
        );
    }
}
