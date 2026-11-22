//! `openbook` — a Questica OpenBook client's budget books, each as the one PDF the product
//! publishes of it.
//!
//! Keyed on a **product**, at two sightings: Pinellas County
//! (`pinellascounty.openbook.questica.com`) and Largo (`largo.openbook.questica.com`). Both
//! publish budgets nowhere else for some years — Largo's site holds one adopted budget, FY2018,
//! and Pinellas's FY25 adopted budget and FY26–31 capital plan are only here
//! (`docs/FIELD-NOTES.md` entry 19).
//!
//! ## Where the books are
//!
//! The site is a script that draws everything from a JSON API on the same host. A book is
//! drawn at `#/budget-book/{key}`, a fragment no server sees; the same route as a path
//! answers the product's error page. Three answers lead to its PDF:
//!
//! 1. `api/home/cards` — the front door's cards. A card whose `cardType` is `BudgetBook`
//!    has the book's **version** id as its `id`.
//! 2. `api/digitalbudgetbookviewer/versionIds/{endpoint}` — the version id for a book's
//!    custom name (`FY27AdoptedBudget`), as a JSON string. A name it does not know answers
//!    the nil GUID.
//! 3. `api/digitalbudgetbookviewer/versions/{id}` — the published version: its name,
//!    contents, and **`publishedPdfHref`**, the whole book as one PDF on Questica's blob
//!    storage (`opendatastorageprodus.blob.core.windows.net/{client}-pdf-{id}/{name}.pdf`).
//!
//! The PDF is the document kept: 12 MB to 366 MB, measured on all fifteen published at the
//! two clients on 2026-11-22. A book whose `showPDFLink` is false publishes none, and its
//! pages are JSON this walk does not read; the walk says which books those are.
//!
//! ## The seed
//!
//! The front door walks every card. A client can turn a book's card off and still link the
//! book from its own site — four of Pinellas's eight are cardless — so a seed may also name
//! one book, by the API address that answers for it:
//!
//! ```text
//! https://pinellascounty.openbook.questica.com/api/digitalbudgetbookviewer/versionIds/FY26-31_CIP
//! https://pinellascounty.openbook.questica.com/api/digitalbudgetbookviewer/versions/c50a9c87-90d2-4b9e-8bda-905d2c8f57d2
//! ```
//!
//! The custom name is the one in the client's own link (`#/budget-book/FY26-31_CIP`); a
//! card's link names the version id. The fragment itself cannot be the seed: the HTTP client
//! records where an answer came from as the request's URI, which has no fragment, so a seed of
//! `…/#/budget-book/FY26-31_CIP` would arrive as the front door and walk the cards instead.
//!
//! ## Cost
//!
//! One request for the cards and one per book, and one for the blob host's `robots.txt` (it
//! answers 400, and no rules are assumed). A book seed's first answer is the seed itself.
//! Largo's front door is 13 requests for 11 books.

use std::fmt;

use futures::future::BoxFuture;
use serde::Deserialize;
use url::Url;

use super::{Addresses, Enumerated, Pass, Seed, Strategy, StrategyDef, Walk};
use crate::domain::Note;
use crate::strategies::{Keyed, Recognition};

pub struct OpenBook;

inventory::submit! { StrategyDef { name: "openbook", it: &OpenBook } }

/// The product, as a recognition names it.
const PRODUCT: &str = "Questica OpenBook";

/// Both are on every OpenBook front door seen: the product's name as the page's title, and
/// its icon.
const TITLE: &str = "<title>OpenBook</title>";
const ICON: &str = "openbook.ico";

/// Where the viewer's API answers for one book.
const VIEWER: &str = "/api/digitalbudgetbookviewer/";

/// The card type a book is listed under.
const BOOK_CARD: &str = "BudgetBook";

/// What `versionIds` answers for a name it does not know.
const NIL: &str = "00000000-0000-0000-0000-000000000000";

/// The cards, or a book's name, at depth 0; a version at 1.
const MAX_DEPTH: usize = 1;

impl Strategy for OpenBook {
    fn name(&self) -> &'static str {
        "openbook"
    }

    fn recognise(&self, seed: &Seed) -> Option<Recognition> {
        let at = seed.final_url()?;
        let book = book_of(&at);
        let recognised = match &book {
            // A book's address, answered as that address answers.
            Some(ask) => ask.understands(&seed.page.bytes),
            None => {
                let page = seed.text();
                page.contains(TITLE) && page.contains(ICON)
            }
        };
        if !recognised {
            return None;
        }
        let r = Recognition::new(self.name(), Keyed::Product(PRODUCT))
            .seeing("client", at.host_str().unwrap_or("?").to_string());
        Some(match book {
            Some(ask) => r.seeing("book", ask.to_string()),
            None => r.seeing(
                "books",
                "every card the front door lists (`api/home/cards`)",
            ),
        })
    }

    fn enumerate<'a>(
        &'a self,
        seed: &'a Seed,
        crawl: &'a dyn Walk,
    ) -> BoxFuture<'a, anyhow::Result<Enumerated>> {
        Box::pin(async move {
            let at = seed
                .final_url()
                .ok_or_else(|| anyhow::anyhow!("the seed does not record where it was served"))?;
            let root = at.join("/")?;

            let mut pass = Pass::new(crawl, "library", MAX_DEPTH);
            pass.note(Note::new("client", root.to_string()));
            // A book seed is that book's first answer, already fetched.
            let mut in_hand = book_of(&at);
            pass.push(in_hand.clone().unwrap_or(Ask::Cards), 0);

            let (mut books, mut pdfs, mut without) = (0u64, 0u64, 0u64);
            while let Some((ask, depth)) = pass.next_to_visit() {
                pass.visiting(ask.to_string());
                let body = if in_hand.as_ref() == Some(&ask) {
                    in_hand = None;
                    seed.page.bytes.clone()
                } else {
                    match pass.walk().get(&ask.url(&root)).await {
                        Ok(f) => f.bytes,
                        Err(refusal) => {
                            pass.refused(&ask, &refusal);
                            continue;
                        }
                    }
                };
                match &ask {
                    Ask::Cards => {
                        let Ok(cards) = serde_json::from_slice::<Vec<Card>>(&body) else {
                            pass.warn(format!("{ask}: the answer is not a list of cards"));
                            continue;
                        };
                        pass.read_surface();
                        for card in cards.into_iter().filter(|c| c.card_type == BOOK_CARD) {
                            pass.push(Ask::Version(card.id.to_ascii_lowercase()), depth + 1);
                        }
                    }
                    Ask::Named(name) => {
                        let Ok(id) = serde_json::from_slice::<String>(&body) else {
                            pass.warn(format!("{ask}: the answer is not a version id"));
                            continue;
                        };
                        // Read and understood, whatever it names: a name nobody published is an
                        // empty answer, not one that could not be had.
                        pass.read_surface();
                        if id == NIL {
                            pass.warn(format!("no book is published as `{name}`"));
                            continue;
                        }
                        pass.push(Ask::Version(id.to_ascii_lowercase()), depth + 1);
                    }
                    Ask::Version(_) => {
                        let Ok(version) = serde_json::from_slice::<Version>(&body) else {
                            pass.warn(format!("{ask}: the answer is not a published book"));
                            continue;
                        };
                        pass.read_surface();
                        books += 1;
                        match version.pdf() {
                            Some(pdf) if !pass.full() => {
                                pdfs += 1;
                                let rules = pass.walk().robots(pdf).await;
                                pass.keep(pdf, &rules);
                            }
                            Some(_) => {}
                            None => {
                                without += 1;
                                pass.warn(format!(
                                    "\"{}\" is published without a PDF; its pages are not \
                                     collected",
                                    version.name
                                ));
                            }
                        }
                    }
                }
            }

            pass.figure("books", books);
            pass.figure("pdfs", pdfs);
            pass.figure("without_pdf", without);
            Ok(pass.finish())
        })
    }

    /// Each address is a whole book as a PDF.
    fn addresses_are(&self) -> Addresses {
        Addresses::Documents
    }
}

/// The one book a seed's address answers for, or `None` for the front door:
/// `…/api/digitalbudgetbookviewer/versionIds/FY26-31_CIP` names a book by its custom name, and
/// `…/versions/{id}` by its version.
fn book_of(at: &Url) -> Option<Ask> {
    let rest = at.path().strip_prefix(VIEWER)?;
    let (kind, key) = rest.split_once('/')?;
    if key.is_empty() || key.contains('/') {
        return None;
    }
    if kind.eq_ignore_ascii_case("versionIds") {
        Some(Ask::Named(key.to_string()))
    } else if kind.eq_ignore_ascii_case("versions") {
        Some(Ask::Version(key.to_ascii_lowercase()))
    } else {
        None
    }
}

/// One request this walk makes.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Ask {
    /// The front door's cards.
    Cards,
    /// The version id behind a book's custom name.
    Named(String),
    /// One published version, for its PDF.
    Version(String),
}

impl Ask {
    fn url(&self, root: &Url) -> String {
        match self {
            Self::Cards => format!("{root}api/home/cards"),
            Self::Named(name) => {
                format!("{root}api/digitalbudgetbookviewer/versionIds/{name}")
            }
            Self::Version(id) => format!("{root}api/digitalbudgetbookviewer/versions/{id}"),
        }
    }

    /// Whether `body` is what this address answers: a version id, or a published version.
    fn understands(&self, body: &[u8]) -> bool {
        match self {
            Self::Cards => serde_json::from_slice::<Vec<Card>>(body).is_ok(),
            Self::Named(_) => serde_json::from_slice::<String>(body).is_ok(),
            Self::Version(_) => serde_json::from_slice::<Version>(body).is_ok(),
        }
    }
}

/// How it reads in a warning, and what [`Pass`] dedups on.
impl fmt::Display for Ask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cards => write!(f, "the front door's cards"),
            Self::Named(name) => write!(f, "book `{name}`"),
            Self::Version(id) => write!(f, "book version {id}"),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Card {
    id: String,
    card_type: String,
}

#[derive(Debug, Deserialize)]
struct Version {
    name: String,
    #[serde(default, rename = "showPDFLink")]
    show_pdf_link: bool,
    #[serde(default, rename = "publishedPdfHref")]
    published_pdf_href: Option<String>,
}

impl Version {
    /// The book's PDF, where the client publishes one.
    fn pdf(&self) -> Option<&str> {
        self.published_pdf_href
            .as_deref()
            .filter(|href| self.show_pdf_link && !href.trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::crawl::tests::{Fake, seed};

    const LARGO: &str = "https://largo.openbook.questica.com/";
    const PINELLAS: &str = "https://pinellascounty.openbook.questica.com/";

    /// The front door's markup, as Largo and Pinellas serve it, cut to what recognition reads.
    const SHELL: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8" />
    <title>OpenBook</title>
    <meta name="description" content="Make your data beautiful, understandable, and accessible to the world." />
    <link rel="shortcut icon" href="https://largo.openbook.questica.com/openbook.ico" type="image/x-icon" />
</head><body></body></html>"#;

    /// Largo's cards on 2026-11-22, cut to two books and a chart.
    const LARGO_CARDS: &str = r#"[
{"id": "53eba3a0-282e-45d1-8590-76fc6f12ed26", "name": "FY 2027 Proposed Annual Budget", "cardDate": "2026-07-02T00:27:04.8591445", "visibility": "Public", "cardType": "BudgetBook", "itemURL": "budget-book/53EBA3A0-282E-45D1-8590-76FC6F12ED26", "budgetYear": 2027},
{"id": "2aa4760f-addb-4bae-9b4e-bb9d7023c112", "name": "FY 2026 Adopted Annual Budget", "cardDate": "2025-12-15T21:20:50.5161986", "visibility": "Public", "cardType": "BudgetBook", "itemURL": "budget-book/2AA4760F-ADDB-4BAE-9B4E-BB9D7023C112", "budgetYear": 2026},
{"id": "83d496b8-b202-4b19-b54c-c616c1d18f7f", "name": "FY 2025-2029 Capital Improvements Program (CIP)", "cardDate": "2024-04-02T19:39:52.161928", "visibility": "Public", "cardType": "Visualization", "itemURL": "visualization/83d496b8-b202-4b19-b54c-c616c1d18f7f", "budgetYear": 0}
]"#;

    /// A published version as the viewer answers it, cut to the fields that matter and a few
    /// that do not.
    fn version(
        id: &str,
        name: &str,
        show: bool,
        pdf: Option<&str>,
        endpoint: Option<&str>,
    ) -> String {
        serde_json::json!({
            "id": id,
            "name": name,
            "isCardDisabled": endpoint.is_some(),
            "showPDFLink": show,
            "visibility": 1,
            "tableOfContents": {"tableOfContentsItems": []},
            "publishedPdfHref": pdf,
            "customEndpoint": endpoint,
        })
        .to_string()
    }

    fn at(root: &str, path: &str) -> String {
        format!("{root}api/digitalbudgetbookviewer/{path}")
    }

    const LARGO_FY27_PROPOSED: &str = "https://opendatastorageprodus.blob.core.windows.net/largo-pdf-53eba3a0-282e-45d1-8590-76fc6f12ed26/fy-2027-proposed-annual-budget.pdf";
    const LARGO_FY26_ADOPTED: &str = "https://opendatastorageprodus.blob.core.windows.net/largo-pdf-2aa4760f-addb-4bae-9b4e-bb9d7023c112/fy-2026-adopted-annual-budget.pdf";

    /// The front door lists its books as cards; each card's id is the version, and each
    /// version names its PDF. A chart's card is not a book.
    #[tokio::test]
    async fn the_front_door_keeps_each_cards_book_as_its_pdf() {
        let s = seed(SHELL, LARGO);
        let crawl = Fake::new([
            (
                format!("{LARGO}api/home/cards").as_str(),
                LARGO_CARDS.to_string(),
            ),
            (
                at(LARGO, "versions/53eba3a0-282e-45d1-8590-76fc6f12ed26").as_str(),
                version(
                    "53eba3a0-282e-45d1-8590-76fc6f12ed26",
                    "FY 2027 Proposed Annual Budget",
                    true,
                    Some(LARGO_FY27_PROPOSED),
                    None,
                ),
            ),
            (
                at(LARGO, "versions/2aa4760f-addb-4bae-9b4e-bb9d7023c112").as_str(),
                version(
                    "2aa4760f-addb-4bae-9b4e-bb9d7023c112",
                    "FY 2026 Adopted Annual Budget",
                    true,
                    Some(LARGO_FY26_ADOPTED),
                    None,
                ),
            ),
        ]);

        let got = OpenBook.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(got.addresses, [LARGO_FY27_PROPOSED, LARGO_FY26_ADOPTED]);
        assert_eq!(
            crawl.requests(),
            3,
            "the cards and two versions; the blob host's rules are the fake's own"
        );
        assert_eq!(got.figures["books"], 2);
        assert_eq!(got.figures["pdfs"], 2);
        assert!(
            !got.truncated && !got.discovery_failed && got.warnings.is_empty(),
            "{:?}",
            got.warnings
        );
    }

    const PINELLAS_FY27: &str = "https://opendatastorageprodus.blob.core.windows.net/pinellascounty-pdf-395fc3b1-e3fe-4419-aa94-1008fbee725a/2027-adopted-budget-book.pdf";

    /// A seed may name one cardless book by the API address for the custom name the client
    /// links it with. That answer is the seed, so the walk asks only for the version.
    #[tokio::test]
    async fn a_book_seed_is_its_own_first_answer() {
        let crawl = Fake::new([(
            at(PINELLAS, "versions/395fc3b1-e3fe-4419-aa94-1008fbee725a").as_str(),
            version(
                "395fc3b1-e3fe-4419-aa94-1008fbee725a",
                "2027 Adopted Budget Book",
                true,
                Some(PINELLAS_FY27),
                Some("FY27AdoptedBudget"),
            ),
        )]);
        let s = seed(
            "\"395fc3b1-e3fe-4419-aa94-1008fbee725a\"",
            &at(PINELLAS, "versionIds/FY27AdoptedBudget"),
        );

        let got = OpenBook.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(got.addresses, [PINELLAS_FY27]);
        assert_eq!(
            crawl.requests(),
            1,
            "the version; the version id was in hand"
        );
        assert!(
            !got.truncated && got.warnings.is_empty(),
            "{:?}",
            got.warnings
        );
    }

    /// A book published without a PDF is said, by name, and nothing is kept for it. Pinellas's
    /// FY26 adopted budget, seeded by its version.
    #[tokio::test]
    async fn a_book_without_a_pdf_is_said_by_name() {
        let s = seed(
            &version(
                "f78adcf6-36a0-409f-b79e-8a36d7abfb0f",
                "FY26 Adopted Operating And Capital Budget",
                false,
                None,
                Some("FY26AdoptedBudget"),
            ),
            &at(PINELLAS, "versions/f78adcf6-36a0-409f-b79e-8a36d7abfb0f"),
        );
        let crawl = Fake::new([]);

        let got = OpenBook.enumerate(&s, &crawl).await.unwrap();

        assert!(got.addresses.is_empty());
        assert_eq!(crawl.requests(), 0);
        assert_eq!(got.figures["without_pdf"], 1);
        assert!(
            got.warnings
                .iter()
                .any(|w| w.contains("FY26 Adopted Operating And Capital Budget")),
            "{:?}",
            got.warnings
        );
        assert!(!got.discovery_failed, "the book was read; it has no PDF");
    }

    /// A name the client does not use answers the nil GUID: an empty answer, said by name.
    #[tokio::test]
    async fn a_name_nobody_published_is_said() {
        let s = seed(
            &format!("\"{NIL}\""),
            &at(PINELLAS, "versionIds/FY99Adopted"),
        );
        let crawl = Fake::new([]);

        let got = OpenBook.enumerate(&s, &crawl).await.unwrap();

        assert!(got.addresses.is_empty());
        assert_eq!(crawl.requests(), 0);
        assert!(
            !got.discovery_failed,
            "the answer was read, and it named no book"
        );
        assert!(got.warnings.iter().any(|w| w.contains("FY99Adopted")));
    }

    /// A book is named by the viewer's API address for it, by custom name or version id, and
    /// any other address is the front door.
    #[test]
    fn a_seed_names_a_book_by_its_api_address() {
        let book = |u: &str| book_of(&Url::parse(u).unwrap());
        assert_eq!(
            book(&at(
                PINELLAS,
                "versions/C50A9C87-90D2-4B9E-8BDA-905D2C8F57D2"
            )),
            Some(Ask::Version("c50a9c87-90d2-4b9e-8bda-905d2c8f57d2".into()))
        );
        assert_eq!(
            book(&at(PINELLAS, "versionIds/FY26-31_CIP")),
            Some(Ask::Named("FY26-31_CIP".into()))
        );
        assert_eq!(book(LARGO), None);
        assert_eq!(
            book("https://pinellascounty.openbook.questica.com/#/budget-book/FY26-31_CIP"),
            None,
            "a fragment never reaches the strategy"
        );
        assert_eq!(
            book(&at(PINELLAS, "pages/7f852511-a527-45ed-91a6-0face05bd002")),
            None
        );
    }

    /// Recognised on the product's own page, or on a book address answering as it does; an
    /// ordinary page is not, and neither is a book address that answers something else.
    #[test]
    fn the_product_is_recognised_on_what_it_serves() {
        let r = OpenBook
            .recognise(&seed(SHELL, LARGO))
            .expect("an OpenBook front door");
        assert_eq!(r.keyed_on, Keyed::Product(PRODUCT));
        assert!(
            OpenBook
                .recognise(&seed(
                    "\"395fc3b1-e3fe-4419-aa94-1008fbee725a\"",
                    &at(PINELLAS, "versionIds/FY27AdoptedBudget")
                ))
                .is_some()
        );
        assert!(
            OpenBook
                .recognise(&seed(
                    "<html><head><title>Budget</title></head></html>",
                    "https://www.largo.com/budget"
                ))
                .is_none()
        );
        // What `versions/` answered on 2026-11-22 for a custom name, which it does not take.
        let refused = r#"{"messages":["The specified budget book version could not be found."],"cause":"The specified budget book version could not be found.","stackTrace":null,"code":null}"#;
        assert!(
            OpenBook
                .recognise(&seed(refused, &at(PINELLAS, "versions/FY27AdoptedBudget")))
                .is_none(),
            "an answer that is not a published book"
        );
    }
}
