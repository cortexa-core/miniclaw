/// Find the largest byte index <= `max` that lies on a UTF-8 character boundary.
pub fn floor_char_boundary(s: &str, max: usize) -> usize {
    if max >= s.len() {
        return s.len();
    }
    let mut i = max;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_floor_char_boundary() {
        let s = "café"; // c(1) a(1) f(1) é(2) = 5 bytes
        assert_eq!(floor_char_boundary(s, 5), 5); // exact end
        assert_eq!(floor_char_boundary(s, 4), 3); // mid-é → snaps back to 'f' end
        assert_eq!(floor_char_boundary(s, 3), 3); // on boundary
        assert_eq!(floor_char_boundary(s, 0), 0); // zero
        assert_eq!(floor_char_boundary(s, 100), 5); // beyond end
    }
}
