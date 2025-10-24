#[cfg(test)]
mod tests {
    use crate::tests::test::{get_report, report_has_error_that_matches};

    #[test]
    fn test_single_matcher_matches_good_regex() {
        assert!(get_report("test", "`id:/test/`").is_valid());
    }

    #[test]
    fn test_single_matcher_matches_bad_regex() {
        let report = get_report("testttt", "`id:/test/`");
        print!("{:?}", report);
        assert!(
            !report.is_valid(),
            "Report should be invalid due to regex mismatch"
        );
        assert!(report_has_error_that_matches(
            &report,
            "Matcher mismatch: input 'testttt' does not"
        ));
    }

    #[test]
    fn test_multiple_matchers() {
        // The schema becomes a paragraph with multiple code nodes, with unique text for each
        // The input is just a single paragraph with a text
        let report = get_report("test example", "`id:/test/` `id:/example/`");

        print!("{:?}", report);
        assert!(
            !report.is_valid(),
            "Report should be invalid due to multiple matchers"
        );
        assert!(report_has_error_that_matches(
            &report,
            "Multiple matchers in a single node are not supported"
        ))
    }
}
