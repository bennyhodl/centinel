//! `rolodex` — the people this corpus names by linking to them.
//!
//! ```text
//! centinel rolodex                     every name, most-named first
//! centinel rolodex --source tampa      the names one source's pages link
//! centinel rolodex --name viera        names holding every word given
//! centinel rolodex show "Luis Viera"   one name: its pages, addresses, accounts and links
//! ```
//!
//! A read of the crumb ledger, gathered by name — see [`crate::rolodex`]. A page whose row was
//! written before mentions were read is counted and named, never guessed at, and
//! `centinel crumbs --rescan` reads it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::prelude::*;
use crate::rolodex::{Appearance, Person, Rolodex};

fn default_max() -> usize {
    25
}

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct RolodexArgs {
    #[command(subcommand)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<RolodexAction>,

    /// Only the names one source's pages link. Omit for the whole corpus.
    #[arg(long)]
    #[serde(default)]
    pub source: Option<String>,

    /// Only names holding every word of this, any case.
    #[arg(long)]
    #[serde(default)]
    pub name: Option<String>,

    /// Names to list before the rest are counted.
    #[arg(long, default_value_t = 25)]
    #[serde(default = "default_max")]
    pub max: usize,
}

/// Hand-written, for the same reason as `CrumbsArgs`: a derived one sets `max` to zero.
impl Default for RolodexArgs {
    fn default() -> Self {
        Self {
            action: None,
            source: None,
            name: None,
            max: default_max(),
        }
    }
}

#[derive(Clone, Debug, clap::Subcommand, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum RolodexAction {
    /// Everything the corpus links under one name.
    Show(PersonArgs),
}

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct PersonArgs {
    /// The name, as listed. Any case.
    #[arg(value_name = "NAME")]
    pub name: String,
}

/// One name in a list: how much links it, and not the pages themselves.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Listed {
    pub name: String,
    pub key: String,
    pub pages: usize,
    /// Per source, how many of its pages named them.
    pub sources: std::collections::BTreeMap<String, usize>,
    pub emails: Vec<String>,
    /// `platform:handle`.
    pub profiles: Vec<String>,
    pub links: usize,
}

impl From<&Person> for Listed {
    fn from(p: &Person) -> Self {
        Self {
            name: p.name.clone(),
            key: p.key.clone(),
            pages: p.pages,
            sources: p
                .sources
                .iter()
                .map(|(s, n)| (s.clone(), n.pages))
                .collect(),
            emails: p.emails.keys().cloned().collect(),
            profiles: p.profiles.keys().cloned().collect(),
            links: p.links.len(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum RolodexReport {
    List {
        /// The Sources whose ledgers were read.
        sources: Vec<String>,
        /// Pages with a ledger row: the denominator.
        pages: usize,
        /// Names matching, before `--max`.
        matched: usize,
        people: Vec<Listed>,
        /// Addresses and handles linked with no name beside them.
        unnamed: usize,
        /// Pages whose row predates mentions, so nobody they name is in this list.
        /// `centinel crumbs --rescan` reads them.
        unread: usize,
    },
    Show {
        /// What was asked for.
        asked: String,
        /// Absent when no page links that name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        person: Option<Person>,
        /// Every source the name comes up in, linked or written, most first.
        sources: Vec<Appearance>,
        /// Whether document text was asked at all: `false` when the store has no index.
        indexed: bool,
        unread: usize,
    },
}

/// The people this corpus names by linking to them: addresses, accounts and pages about
/// them — and with `show`, every source a name comes up in, linked or written.
#[op(group = "corpus")]
pub async fn rolodex(ctx: &Ctx, args: RolodexArgs) -> anyhow::Result<RolodexReport> {
    let rolodex = Rolodex::read(&ctx.store, args.source.as_deref()).await?;
    if let Some(RolodexAction::Show(which)) = args.action {
        return Ok(RolodexReport::Show {
            sources: rolodex.appearances(&ctx.store, &which.name).await?,
            indexed: ctx.store.index_path().exists(),
            person: rolodex.person(&which.name).cloned(),
            asked: which.name,
            unread: rolodex.unread,
        });
    }

    let rolodex = match args.name.as_deref() {
        Some(query) => rolodex.matching(query),
        None => rolodex,
    };
    Ok(RolodexReport::List {
        matched: rolodex.people.len(),
        people: rolodex
            .people
            .iter()
            .take(args.max)
            .map(Listed::from)
            .collect(),
        sources: rolodex.sources,
        pages: rolodex.pages,
        unnamed: rolodex.unnamed,
        unread: rolodex.unread,
    })
}

// -----------------------------------------------------------------------------------------
// How it reads
// -----------------------------------------------------------------------------------------

/// The one line naming what is missing and the command that reads it.
fn unread_note(p: &mut Painter<'_>, unread: usize) -> std::io::Result<()> {
    match unread {
        0 => Ok(()),
        n => p.marked(
            Mark::Warn,
            p.paint(
                &format!(
                    "{} were recorded before names were read, so nobody they link is here — \
                     `centinel crumbs --rescan` reads them once",
                    render::plural(n, "page", "pages"),
                ),
                Ink::Dim,
            ),
        ),
    }
}

/// Every source a name comes up in, and under each the pages that link it.
fn appearances(
    p: &mut Painter<'_>,
    sources: &[Appearance],
    indexed: bool,
    person: Option<&Person>,
) -> std::io::Result<()> {
    p.section("sources")?;
    p.nest(|p| {
        if sources.is_empty() {
            p.wrapped("no source names them.", Ink::Dim)?;
        }
        for a in sources {
            let mut how = Vec::new();
            if a.linked > 0 {
                how.push(format!(
                    "linked from {}",
                    render::plural(a.linked, "page", "pages")
                ));
            }
            if let Some(named) = a.named.filter(|n| *n > 0) {
                how.push(format!(
                    "named in {}",
                    render::plural(named, "document", "documents")
                ));
            }
            p.line(format!(
                "{}  {}",
                p.paint(&a.source, Ink::Bold),
                p.paint(&how.join(" · "), Ink::Dim)
            ))?;
            let Some(pages) = person.and_then(|person| person.sources.get(&a.source)) else {
                continue;
            };
            p.nest(|p| {
                for address in &pages.examples {
                    p.line(render::truncate(address, p.width().saturating_sub(8)))?;
                }
                if pages.pages > pages.examples.len() {
                    p.line(p.paint(
                        &format!("… and {} more", pages.pages - pages.examples.len()),
                        Ink::Dim,
                    ))?;
                }
                Ok(())
            })?;
        }
        if !indexed {
            p.marked(
                Mark::Warn,
                p.paint(
                    "document text was not searched: this store has no index — \
                     `centinel index` builds it",
                    Ink::Dim,
                ),
            )?;
        }
        Ok(())
    })
}

impl Render for RolodexReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        match self {
            Self::List {
                sources,
                pages,
                matched,
                people,
                unnamed,
                unread,
            } => {
                p.title(
                    "rolodex",
                    &format!(
                        "{} on {} in {}",
                        render::plural(*matched, "name", "names"),
                        render::plural(*pages, "page", "pages"),
                        render::plural(sources.len(), "source", "sources"),
                    ),
                )?;
                p.nest(|p| {
                    if people.is_empty() {
                        p.wrapped(
                            match pages {
                                0 => "nothing collected yet. Run `centinel run` first.",
                                _ => "no link in these pages names anybody.",
                            },
                            Ink::Dim,
                        )?;
                    } else {
                        let mut table = Table::new(&[
                            ("name", Align::Left),
                            ("pages", Align::Right),
                            ("sources", Align::Left),
                            ("linked", Align::Left),
                        ]);
                        for person in people {
                            let ids: Vec<&String> =
                                person.emails.iter().chain(&person.profiles).collect();
                            let linked = match ids.as_slice() {
                                [] => String::new(),
                                [one] => one.to_string(),
                                [first, rest @ ..] => format!("{first} +{}", rest.len()),
                            };
                            table.push(vec![
                                Cell::new(render::truncate(&person.name, 32), Ink::Plain),
                                Cell::new(render::count(person.pages as u64), Ink::Plain),
                                Cell::dim(render::truncate(
                                    &person
                                        .sources
                                        .keys()
                                        .cloned()
                                        .collect::<Vec<_>>()
                                        .join(", "),
                                    24,
                                )),
                                Cell::dim(render::truncate(&linked, 48)),
                            ]);
                        }
                        p.table(&table)?;
                        if *matched > people.len() {
                            p.note(format!(
                                "… and {} more, raise --max or narrow with --name",
                                render::count((*matched - people.len()) as u64)
                            ))?;
                        }
                    }
                    if *unnamed > 0 {
                        p.note(format!(
                            "{} linked with no name beside them",
                            render::plural(*unnamed, "address or handle", "addresses and handles")
                        ))?;
                    }
                    unread_note(p, *unread)
                })?;
                if let Some(first) = people.first() {
                    p.blank()?;
                    p.wrapped(
                        &format!("centinel rolodex show \"{}\"", first.name),
                        Ink::Bold,
                    )?;
                }
                Ok(())
            }

            Self::Show {
                asked,
                person,
                sources,
                indexed,
                unread,
            } => {
                let Some(person) = person else {
                    p.title(asked, "")?;
                    return p.nest(|p| {
                        p.wrapped("no link in this corpus names them.", Ink::Dim)?;
                        appearances(p, sources, *indexed, None)?;
                        unread_note(p, *unread)
                    });
                };
                p.title(
                    &person.name,
                    &format!(
                        "{} in {}",
                        render::plural(person.pages, "page", "pages"),
                        render::plural(person.sources.len(), "source", "sources"),
                    ),
                )?;
                p.nest(|p| {
                    if person.spellings.len() > 1 {
                        p.wrapped(
                            &format!(
                                "also written {}",
                                person
                                    .spellings
                                    .keys()
                                    .filter(|s| **s != person.name)
                                    .cloned()
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                            Ink::Dim,
                        )?;
                    }
                    if !person.emails.is_empty() {
                        p.section("addresses")?;
                        p.nest(|p| {
                            for (email, pages) in &person.emails {
                                p.line(format!(
                                    "{email}  {}",
                                    p.paint(&render::plural(*pages, "page", "pages"), Ink::Dim)
                                ))?;
                            }
                            Ok(())
                        })?;
                    }
                    if !person.profiles.is_empty() {
                        p.section("accounts")?;
                        p.nest(|p| {
                            for profile in person.profiles.values() {
                                p.line(format!(
                                    "{} {}  {}",
                                    profile.platform,
                                    p.paint(&profile.url, Ink::Plain),
                                    p.paint(
                                        &render::plural(profile.pages, "page", "pages"),
                                        Ink::Dim
                                    )
                                ))?;
                            }
                            Ok(())
                        })?;
                    }
                    if !person.links.is_empty() {
                        p.section("pages about them")?;
                        p.nest(|p| {
                            for (url, pages) in &person.links {
                                p.line(format!(
                                    "{}  {}",
                                    render::truncate(url, p.width().saturating_sub(16)),
                                    p.paint(&render::plural(*pages, "page", "pages"), Ink::Dim)
                                ))?;
                            }
                            Ok(())
                        })?;
                    }
                    appearances(p, sources, *indexed, Some(person))?;
                    unread_note(p, *unread)
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{ContentKind, SNIFF_BYTES};
    use crate::crumbs::Ledger;
    use crate::domain::Resource;
    use crate::store::Store;
    use std::collections::BTreeMap;

    async fn ctx() -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let ctx = Ctx::new(store);
        crate::catalog::Sources::new(&ctx.store)
            .add(&crate::config::SourceConfig::site(
                "tampa",
                "https://www.tampa.gov",
            ))
            .await
            .unwrap();
        (dir, ctx)
    }

    /// A page as `collect` leaves it: an Observation in the log and a row in the ledger.
    async fn collected(ctx: &Ctx, address: &str, html: &str) {
        let resource = Resource::new(SourceId::new("tampa").unwrap(), address);
        let obs = ctx
            .store
            .record_observation(
                &resource,
                html.as_bytes(),
                "2026-10-01T00:00:00Z".parse().unwrap(),
                BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
            )
            .await
            .unwrap();
        let bytes = html.as_bytes();
        let kind = ContentKind::classify(&obs.meta, &bytes[..bytes.len().min(SNIFF_BYTES)]);
        Ledger::new(&ctx.store, resource.source.clone())
            .record(&resource, bytes, kind, &obs)
            .await;
    }

    /// Collected pages in, a name with its address and account out — and a page collected
    /// later shows up without anyone clearing a cache.
    #[tokio::test]
    async fn a_collected_page_puts_the_people_it_links_in_the_rolodex() {
        let (_d, ctx) = ctx().await;
        collected(
            &ctx,
            "https://www.tampa.gov/council",
            r#"<a href="mailto:lviera@tampagov.net">Luis Viera</a>
               <a href="https://x.com/LuisVieraTampa">Council Member Luis Viera</a>
               <a href="mailto:clerk@tampagov.net">Email the clerk</a>"#,
        )
        .await;

        let RolodexReport::List {
            people, unnamed, ..
        } = rolodex(&ctx, RolodexArgs::default()).await.unwrap()
        else {
            panic!("expected a list")
        };
        assert_eq!(people.len(), 1);
        assert_eq!(people[0].name, "Luis Viera");
        assert_eq!(people[0].emails, ["lviera@tampagov.net"]);
        assert_eq!(people[0].profiles, ["x:luisvieratampa"]);
        assert_eq!(unnamed, 1, "the clerk's address is kept, under no name");

        collected(
            &ctx,
            "https://www.tampa.gov/staff",
            r#"<a href="/staff/jane-doe">Jane Doe</a>"#,
        )
        .await;
        let RolodexReport::Show { person, .. } = rolodex(
            &ctx,
            RolodexArgs {
                action: Some(RolodexAction::Show(PersonArgs {
                    name: "jane DOE".into(),
                })),
                ..Default::default()
            },
        )
        .await
        .unwrap() else {
            panic!("expected one person")
        };
        let jane = person.expect("the second page is read on the next ask");
        assert_eq!(
            jane.links.keys().collect::<Vec<_>>(),
            ["https://www.tampa.gov/staff/jane-doe"]
        );
    }

    /// A page in the log with no ledger row — collected before the ledger, or ingested —
    /// is a page nobody has read for names. It is counted, so an old corpus does not read
    /// as a corpus of nobody, and a rescan reads it.
    #[tokio::test]
    async fn a_page_nobody_has_read_for_names_is_counted_until_a_rescan() {
        let (_d, ctx) = ctx().await;
        let html = r#"<a href="mailto:jdoe@tampagov.net">Jane Doe</a>"#;
        ctx.store
            .record_observation(
                &Resource::new(SourceId::new("tampa").unwrap(), "https://www.tampa.gov/old"),
                html.as_bytes(),
                "2026-10-01T00:00:00Z".parse().unwrap(),
                BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
            )
            .await
            .unwrap();

        let RolodexReport::List {
            pages,
            unread,
            people,
            ..
        } = rolodex(&ctx, RolodexArgs::default()).await.unwrap()
        else {
            panic!("expected a list")
        };
        assert_eq!((pages, unread, people.len()), (1, 1, 0));

        crate::ops::crumbs(
            &ctx,
            crate::ops::CrumbsArgs {
                rescan: true,
                ..Default::default()
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();
        let RolodexReport::List { unread, people, .. } =
            rolodex(&ctx, RolodexArgs::default()).await.unwrap()
        else {
            panic!("expected a list")
        };
        assert_eq!(unread, 0);
        assert_eq!(people[0].name, "Jane Doe");
        assert_eq!(people[0].emails, ["jdoe@tampagov.net"]);
    }

    /// Text written into the index under one source, as `index` would leave it.
    fn written(ctx: &Ctx, source: &str, address: &str, text: &str) {
        let mut index = crate::index::Index::open(ctx.store.index_path()).unwrap();
        let chunk = crate::chunk::Chunk::new(text.to_string(), 0, String::new(), 0);
        index
            .insert(
                &chunk,
                &crate::index::Placement {
                    source: source.into(),
                    resource: address.into(),
                    blob_sha: "0".repeat(64),
                    derived_sha: "1".repeat(64),
                    ordinal: 0,
                    heading: String::new(),
                    char_start: 0,
                    char_end: text.len(),
                    observed_at: "2026-10-01T00:00:00Z".into(),
                    tool: "test".into(),
                    title: None,
                },
            )
            .unwrap();
    }

    fn show(name: &str) -> RolodexArgs {
        RolodexArgs {
            action: Some(RolodexAction::Show(PersonArgs { name: name.into() })),
            ..Default::default()
        }
    }

    /// Every source a name comes up in: the one whose pages link it, and the one whose
    /// minutes only write it — and a name nobody links still has the sources that write it.
    #[tokio::test]
    async fn show_names_every_source_a_name_comes_up_in() {
        let (_d, ctx) = ctx().await;
        crate::catalog::Sources::new(&ctx.store)
            .add(&crate::config::SourceConfig::channel(
                "cttv",
                "https://www.youtube.com/@CityofTampa",
            ))
            .await
            .unwrap();
        collected(
            &ctx,
            "https://www.tampa.gov/council",
            r#"<a href="/council/viera">Council Member Luis Viera</a>"#,
        )
        .await;

        // No index yet: text was not asked, which is not the same as nowhere.
        let RolodexReport::Show {
            sources, indexed, ..
        } = rolodex(&ctx, show("Luis Viera")).await.unwrap()
        else {
            panic!("expected one person")
        };
        assert!(!indexed);
        assert_eq!(
            sources,
            [Appearance {
                source: "tampa".into(),
                linked: 1,
                named: None
            }]
        );

        written(
            &ctx,
            "tampa",
            "https://www.tampa.gov/minutes",
            "Council Member Luis Viera moved to approve.",
        );
        written(
            &ctx,
            "cttv",
            "https://www.youtube.com/watch?v=1",
            "Luis Viera asked about the budget, and Jane Roe answered.",
        );

        let RolodexReport::Show { sources, .. } = rolodex(&ctx, show("Luis Viera")).await.unwrap()
        else {
            panic!("expected one person")
        };
        assert_eq!(
            sources,
            [
                Appearance {
                    source: "tampa".into(),
                    linked: 1,
                    named: Some(1)
                },
                Appearance {
                    source: "cttv".into(),
                    linked: 0,
                    named: Some(1)
                },
            ]
        );

        let RolodexReport::Show {
            person, sources, ..
        } = rolodex(&ctx, show("Jane Roe")).await.unwrap()
        else {
            panic!("expected one person")
        };
        assert!(person.is_none(), "no link names her");
        assert_eq!(
            sources,
            [Appearance {
                source: "cttv".into(),
                linked: 0,
                named: Some(1)
            }]
        );
    }

    /// A page re-collected with new bytes and no new row — what `ingest` leaves — no longer
    /// names who its old bytes named. It is a page nobody has read for names, until a
    /// rescan reads the bytes it serves now.
    #[tokio::test]
    async fn a_page_that_changed_since_its_row_names_nobody_until_it_is_read_again() {
        let (_d, ctx) = ctx().await;
        let address = "https://www.tampa.gov/staff";
        collected(
            &ctx,
            address,
            r#"<a href="mailto:jdoe@tampagov.net">Jane Doe</a>"#,
        )
        .await;
        let html = r#"<a href="mailto:broe@tampagov.net">Bob Roe</a>"#;
        ctx.store
            .record_observation(
                &Resource::new(SourceId::new("tampa").unwrap(), address),
                html.as_bytes(),
                "2026-10-02T00:00:00Z".parse().unwrap(),
                BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
            )
            .await
            .unwrap();

        let RolodexReport::List {
            people,
            pages,
            unread,
            ..
        } = rolodex(&ctx, RolodexArgs::default()).await.unwrap()
        else {
            panic!("expected a list")
        };
        assert!(
            people.is_empty(),
            "the page no longer names Jane: {people:?}"
        );
        assert_eq!((pages, unread), (1, 1));

        crate::ops::crumbs(
            &ctx,
            crate::ops::CrumbsArgs {
                rescan: true,
                ..Default::default()
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();
        let RolodexReport::List { people, unread, .. } =
            rolodex(&ctx, RolodexArgs::default()).await.unwrap()
        else {
            panic!("expected a list")
        };
        assert_eq!(unread, 0);
        assert_eq!(
            people.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["Bob Roe"]
        );
    }

    #[tokio::test]
    async fn an_unknown_source_is_an_error_not_an_empty_rolodex() {
        let (_d, ctx) = ctx().await;
        let err = rolodex(
            &ctx,
            RolodexArgs {
                source: Some("tampaa".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("did you mean `tampa`"), "{err}");
    }
}
