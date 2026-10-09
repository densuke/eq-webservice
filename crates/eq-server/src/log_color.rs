//! ログの ANSI 色付けを付けるかの判定 (Issue #208)。

/// 端末に出すときだけ色を付ける。NO_COLOR が空でなければ付けない。
pub fn use_ansi(is_terminal: bool, no_color: Option<&str>) -> bool {
    is_terminal && no_color.is_none_or(str::is_empty)
}

#[cfg(test)]
mod tests {
    use super::use_ansi;

    #[test]
    fn terminal_without_no_color_is_colored() {
        assert!(use_ansi(true, None));
        assert!(use_ansi(true, Some("")));
    }

    #[test]
    fn non_terminal_is_never_colored() {
        assert!(!use_ansi(false, None));
        assert!(!use_ansi(false, Some("")));
    }

    #[test]
    fn non_empty_no_color_disables() {
        assert!(!use_ansi(true, Some("1")));
    }
}
