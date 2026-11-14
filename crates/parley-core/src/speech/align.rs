//! Word alignment of a speaking check (docs/modules/10-speaking-and-pronunciation.md): the edit
//! distance of Wagner and Fischer (1974) over word keys with unit costs, and the edit script read
//! back from its table.

/// One step of the edit script: Match and Substitute pair expected word `i` with heard word `j`;
/// Delete is expected word `i` missed; Insert is heard word `j` said in addition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    Match(usize, usize),
    Substitute(usize, usize),
    Delete(usize),
    Insert(usize),
}

/// The edit script from the start. The table holds the least number of edits for every pair of
/// prefixes; the script is read back from the end, and where several steps give the same cost it
/// prefers the diagonal (a match or a substitution), then Delete, then Insert, so a word said
/// differently is reported as heard differently rather than as missed and extra.
pub fn align(expected: &[String], heard: &[String]) -> Vec<Edit> {
    let n = expected.len();
    let m = heard.len();
    let width = m + 1;
    // d[i * width + j]: edits between the first i expected and the first j heard words.
    let mut d = vec![0usize; (n + 1) * width];
    for i in 0..=n {
        d[i * width] = i;
    }
    for (j, cell) in d.iter_mut().enumerate().take(m + 1) {
        *cell = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(expected[i - 1] != heard[j - 1]);
            let diagonal = d[(i - 1) * width + (j - 1)] + cost;
            let delete = d[(i - 1) * width + j] + 1;
            let insert = d[i * width + (j - 1)] + 1;
            d[i * width + j] = diagonal.min(delete).min(insert);
        }
    }
    let mut script = Vec::with_capacity(n.max(m));
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        let here = d[i * width + j];
        if i > 0 && j > 0 {
            let same = expected[i - 1] == heard[j - 1];
            if here == d[(i - 1) * width + (j - 1)] + usize::from(!same) {
                script.push(if same {
                    Edit::Match(i - 1, j - 1)
                } else {
                    Edit::Substitute(i - 1, j - 1)
                });
                i -= 1;
                j -= 1;
                continue;
            }
        }
        if i > 0 && here == d[(i - 1) * width + j] + 1 {
            script.push(Edit::Delete(i - 1));
            i -= 1;
        } else {
            script.push(Edit::Insert(j - 1));
            j -= 1;
        }
    }
    script.reverse();
    script
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn the_spec_cases_align_as_computed_by_hand() {
        use Edit::*;
        assert_eq!(
            align(&words("yo soy estudiante"), &words("yo es estudiante")),
            [Match(0, 0), Substitute(1, 1), Match(2, 2)]
        );
        assert_eq!(
            align(
                &words("quiero un cafe con leche"),
                &words("quiero cafe con leche")
            ),
            [
                Match(0, 0),
                Delete(1),
                Match(2, 1),
                Match(3, 2),
                Match(4, 3)
            ]
        );
        assert_eq!(
            align(&words("buenos dias"), &words("buenos dias senor")),
            [Match(0, 0), Match(1, 1), Insert(2)]
        );
        assert_eq!(align(&words("a b"), &[]), [Delete(0), Delete(1)]);
        assert_eq!(align(&[], &words("a")), [Insert(0)]);
        assert!(align(&[], &[]).is_empty());
    }

    #[test]
    fn ties_prefer_the_diagonal_then_delete() {
        use Edit::*;
        // One word for two: a substitution and an insertion or a deletion, the diagonal first.
        assert_eq!(
            align(&words("a"), &words("x y")),
            [Insert(0), Substitute(0, 1)]
        );
        assert_eq!(
            align(&words("x y"), &words("a")),
            [Delete(0), Substitute(1, 0)]
        );
        assert_eq!(
            align(&words("a b"), &words("c d")),
            [Substitute(0, 0), Substitute(1, 1)]
        );
    }
}
