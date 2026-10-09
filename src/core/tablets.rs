// Name, width and height (mm) of every tablet OpenTabletDriver supports, generated from its
// tablet configurations by `cargo run --example update_tablets -- <OTD checkout>`.
const TABLETS: &str = include_str!("tablets.tsv");

pub fn tablet_size(name: &str) -> Option<(f64, f64)> {
    TABLETS.lines().find_map(|line| {
        let mut fields = line.split('\t');
        if fields.next()? != name {
            return None;
        }
        Some((fields.next()?.parse().ok()?, fields.next()?.parse().ok()?))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_tablet_has_its_size() {
        assert_eq!(tablet_size("Wacom CTL-4100"), Some((152.0, 95.0)));
    }

    #[test]
    fn unknown_tablet_has_no_size() {
        assert_eq!(tablet_size("Not A Tablet"), None);
    }

    #[test]
    fn every_bundled_tablet_has_a_positive_size() {
        for line in TABLETS.lines() {
            let fields: Vec<&str> = line.split('\t').collect();
            assert_eq!(fields.len(), 3, "{line:?}");
            let size = tablet_size(fields[0]).unwrap();
            assert!(size.0 > 0.0 && size.1 > 0.0, "{line:?}");
        }
    }
}
