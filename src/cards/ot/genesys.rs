mod search;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};

use search::CardSearch;

#[derive(Debug, Default)]
pub(super) struct GenesysPoints {
    entries: HashMap<i64, i64>,
}

impl GenesysPoints {
    pub(super) fn for_card(&self, id: i64) -> i64 {
        self.entries.get(&id).copied().unwrap_or(0)
    }

    fn insert(&mut self, id: i64, points: i64) -> Result<()> {
        ensure!(id > 0, "Genesys card ID must be positive: {id}");
        if let Some(previous) = self.entries.get(&id) {
            ensure!(
                *previous == points,
                "conflicting Genesys points for card {id}: {previous} and {points}"
            );
        }
        self.entries.insert(id, points);
        Ok(())
    }

    pub(super) fn inherit_aliases(
        &mut self,
        aliases: impl IntoIterator<Item = (i64, i64)>,
    ) -> Result<()> {
        let mut neighbors = BTreeMap::<i64, Vec<i64>>::new();
        for (id, alias) in aliases {
            if id <= 0 || alias <= 0 || id == alias {
                continue;
            }
            neighbors.entry(id).or_default().push(alias);
            neighbors.entry(alias).or_default().push(id);
        }

        let mut visited = HashSet::new();
        for &id in neighbors.keys() {
            if visited.contains(&id) {
                continue;
            }
            let mut pending = vec![id];
            let mut group = Vec::new();
            let mut known = None;
            while let Some(id) = pending.pop() {
                if !visited.insert(id) {
                    continue;
                }
                group.push(id);
                if let Some(&points) = self.entries.get(&id) {
                    if let Some((other_id, previous)) = known {
                        ensure!(
                            previous == points,
                            "conflicting Genesys points in alias group: card {other_id} has {previous}, card {id} has {points}"
                        );
                    }
                    known = Some((id, points));
                }
                pending.extend(&neighbors[&id]);
            }
            // Keep unlisted cards absent until every explicit value has been resolved.
            if let Some((_, points)) = known {
                for id in group {
                    self.entries.insert(id, points);
                }
            }
        }
        Ok(())
    }
}

pub(super) fn read_genesys_points(path: &Path) -> Result<GenesysPoints> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read Genesys list {}", path.display()))?;
    let list = parse_genesys_list(&text)
        .with_context(|| format!("failed to parse Genesys list {}", path.display()))?;
    if list.unmatched.is_empty() {
        return Ok(list.points);
    }

    let search = CardSearch::new(crate::endpoints::endpoints()?.card_search_url())?;
    list.resolve_unmatched(|name| search.resolve(name))
}

#[derive(Debug)]
struct NamedPoints {
    name: String,
    points: i64,
}

#[derive(Debug)]
struct GenesysList {
    points: GenesysPoints,
    unmatched: Vec<NamedPoints>,
}

impl GenesysList {
    fn resolve_unmatched(
        mut self,
        mut resolve: impl FnMut(&str) -> Result<i64>,
    ) -> Result<GenesysPoints> {
        for entry in self.unmatched {
            let id = resolve(&entry.name).with_context(|| {
                format!("failed to resolve unmatched Genesys card {:?}", entry.name)
            })?;
            self.points.insert(id, entry.points)?;
        }
        Ok(self.points)
    }
}

#[derive(Clone, Copy)]
enum Section {
    Points,
    Disabled,
}

fn parse_genesys_list(text: &str) -> Result<GenesysList> {
    let mut list = GenesysList {
        points: GenesysPoints::default(),
        unmatched: Vec::new(),
    };
    let mut section = None;
    let mut saw_disabled = false;

    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if let Some(comment) = line.strip_prefix('#') {
            let comment = comment.trim();
            if comment.eq_ignore_ascii_case("genesys points") {
                ensure!(section.is_none(), "duplicate Genesys points section");
                section = Some(Section::Points);
            } else if section.is_some() {
                if comment.eq_ignore_ascii_case("Disable Pendulum and Link monsters") {
                    ensure!(!saw_disabled, "duplicate Genesys disabled section");
                    section = Some(Section::Disabled);
                    saw_disabled = true;
                } else if let Some(entry) = comment.strip_prefix("[UNMATCHED]") {
                    list.unmatched.push(parse_unmatched(entry).with_context(|| {
                        format!(
                            "invalid unmatched Genesys entry on line {}: {line}",
                            index + 1
                        )
                    })?);
                }
            }
            continue;
        }
        let Some(section) = section else {
            continue;
        };
        // Only consume the first (latest) list if upstream appends older lists.
        if line.starts_with('!') {
            break;
        }
        if line.is_empty() {
            continue;
        }
        let (id, points) = parse_entry(line, section)
            .with_context(|| format!("invalid Genesys entry on line {}: {line}", index + 1))?;
        list.points.insert(id, points)?;
    }

    ensure!(section.is_some(), "missing Genesys points section");
    ensure!(saw_disabled, "missing Genesys disabled section");
    Ok(list)
}

fn parse_entry(line: &str, section: Section) -> Result<(i64, i64)> {
    let entry = line.split_once("--").map_or(line, |(entry, _)| entry);
    let parts = entry.split_whitespace().collect::<Vec<_>>();
    let (id, points) = match (section, parts.as_slice()) {
        (Section::Points, [id, "$genesys", points]) => (*id, parse_points(points)?),
        (Section::Disabled, [id, count]) => {
            parse_points(count)?;
            (*id, -1)
        }
        _ => anyhow::bail!("unexpected Genesys entry format"),
    };
    Ok((id.parse().context("invalid Genesys card ID")?, points))
}

fn parse_points(value: &str) -> Result<i64> {
    let points = value.parse().context("invalid Genesys points")?;
    ensure!(points >= 0, "Genesys source points must be non-negative");
    Ok(points)
}

fn parse_unmatched(entry: &str) -> Result<NamedPoints> {
    let (name, value) = entry
        .rsplit_once("->")
        .context("missing name/points separator")?;
    let name = name.trim();
    ensure!(!name.is_empty(), "missing Genesys card name");
    let value = value
        .split_once("(normalized:")
        .map_or(value, |(value, _)| value);
    Ok(NamedPoints {
        name: name.to_string(),
        points: parse_points(value.trim())?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = "\
        !2026.09 TCG Genesys
        $genesys 100
        999 0 -- ignored before points
        # Genesys points
        00010 $genesys 20 -- A
        20 $genesys 100 -- B
        21 $genesys 0 -- Explicit zero
        # Disable Pendulum and Link monsters
        30 0
        40 0 -- Disabled
        # --- Unmatched cards (by name) ---
        # [UNMATCHED] Exstellarknight Constellar Ptolemy O7 -> 5  (normalized: exstellarknightconstellarptolemyo7)
        ";

    #[test]
    fn parses_sections_and_resolves_unmatched_names() {
        let list = parse_genesys_list(LIST).unwrap();
        let points = list
            .resolve_unmatched(|name| {
                assert_eq!(name, "Exstellarknight Constellar Ptolemy O7");
                Ok(6195332)
            })
            .unwrap();

        for (id, expected) in [
            (10, 20),
            (20, 100),
            (21, 0),
            (30, -1),
            (40, -1),
            (6195332, 5),
            (999, 0),
            (888, 0),
        ] {
            assert_eq!(points.for_card(id), expected, "card {id}");
        }
    }

    #[test]
    fn accepts_case_insensitive_markers_and_crlf() {
        let list = parse_genesys_list(
            "# genesys points\r\n10 $genesys 2\r\n# disable pendulum and link monsters\r\n30 0\r\n",
        )
        .unwrap();
        assert_eq!(list.points.for_card(10), 2);
        assert_eq!(list.points.for_card(30), -1);
    }

    #[test]
    fn ignores_older_lists() {
        let list = parse_genesys_list(&format!(
            "{LIST}\n!Old Genesys\n# Genesys points\n10 $genesys 90\n"
        ))
        .unwrap();
        assert_eq!(list.points.for_card(10), 20);
    }

    #[test]
    fn inherits_points_in_both_directions_across_alias_groups() {
        let mut points = parse_genesys_list(LIST).unwrap().points;
        points
            .inherit_aliases([
                (11, 10),
                (20, 22),
                (23, 22),
                (22, 24),
                (24, 20),
                (30, 31),
                (32, 31),
                (21, 25),
                (50, 51),
                (60, 60),
                (70, 0),
            ])
            .unwrap();

        for (id, expected) in [
            (10, 20),
            (11, 20),
            (20, 100),
            (22, 100),
            (23, 100),
            (24, 100),
            (30, -1),
            (31, -1),
            (32, -1),
            (25, 0),
            (50, 0),
            (51, 0),
            (60, 0),
            (70, 0),
        ] {
            assert_eq!(points.for_card(id), expected, "card {id}");
        }
        assert_eq!(points.entries.get(&25), Some(&0));
        assert!(!points.entries.contains_key(&50));
    }

    #[test]
    fn database_aliases_share_listed_and_resolved_points() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "create table datas (id integer, alias integer);
             insert into datas values
                (10, 12), (12, 0), (13, 12),
                (30, 31), (31, 0),
                (6195333, 6195332), (0, 10), (99, 0);",
            )
            .unwrap();
        let mut points = parse_genesys_list(LIST)
            .unwrap()
            .resolve_unmatched(|_| Ok(6195332))
            .unwrap();
        super::super::inherit_genesys_aliases(&connection, &mut points).unwrap();

        for (id, expected) in [(12, 20), (13, 20), (31, -1), (6195333, 5), (0, 0), (99, 0)] {
            assert_eq!(points.for_card(id), expected, "card {id}");
        }
    }

    #[test]
    fn rejects_conflicting_explicit_alias_values_including_zero() {
        for aliases in [[(10, 20)], [(10, 21)], [(10, 30)]] {
            let mut points = parse_genesys_list(LIST).unwrap().points;
            assert!(points.inherit_aliases(aliases).is_err());
        }
        let mut points = parse_genesys_list(LIST).unwrap().points;
        points.inherit_aliases([(30, 40)]).unwrap();
    }

    #[test]
    fn rejects_malformed_lists_and_conflicting_duplicates() {
        for text in [
            "",
            "# Genesys points\n10 $genesys 5",
            "# Disable Pendulum and Link monsters\n30 0",
        ] {
            assert!(parse_genesys_list(text).is_err(), "{text}");
        }
        for entry in [
            "bad",
            "0 $genesys 5",
            "-1 $genesys 5",
            "10 $genesys -5",
            "10 $genesys 1.5",
            "10 5",
            "10 $genesys 5 extra",
            "10 $genesys 5\n10 $genesys 6",
        ] {
            let text =
                format!("# Genesys points\n{entry}\n# Disable Pendulum and Link monsters\n30 0");
            assert!(parse_genesys_list(&text).is_err(), "{entry}");
        }
        assert!(parse_genesys_list(&LIST.replace("30 0", "30 invalid")).is_err());
        assert!(parse_genesys_list(&LIST.replace("-> 5", "-> nope")).is_err());
    }

    #[test]
    fn propagates_name_lookup_errors_and_conflicts() {
        let error = parse_genesys_list(LIST)
            .unwrap()
            .resolve_unmatched(|_| anyhow::bail!("search unavailable"))
            .unwrap_err();
        assert!(format!("{error:#}").contains("Exstellarknight"));
        assert!(format!("{error:#}").contains("search unavailable"));
        assert!(
            parse_genesys_list(LIST)
                .unwrap()
                .resolve_unmatched(|_| Ok(10))
                .is_err()
        );
        assert!(
            parse_genesys_list(LIST)
                .unwrap()
                .resolve_unmatched(|_| Ok(0))
                .is_err()
        );
    }
}
