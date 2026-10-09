//! The questions Centinel ships with.
//!
//! A new store's `workspace/questions.jsonl` is seeded from these the first time anything
//! asks for the saved questions, and from then on the file is the only owner: the operator
//! edits it, in the web workspace or by hand, and nothing here is consulted again except
//! to offer a preset back for adding. The CLI, the pipeline stage, the web page and search
//! therefore all read one set.
//!
//! ## How the questions are written
//!
//! What held up in other Jev classifiers, and in the September trial on this corpus:
//!
//! - A **choice** for "what kind of thing is this", with the junk kinds beside the useful
//!   kinds so that junk competes with records for the same probability.
//! - An `other` option in every choice, so a document that fits nothing is not forced into
//!   a wrong option.
//! - Options defined by purpose, with the words a reader would see on the page, and the
//!   one or two options each is most likely confused with.
//! - A **yes-or-no** question (a noul) for anything a document can have several of —
//!   topics, audiences — because a choice would make them compete.
//!
//! Thresholds here are starting policy. They are meant to be moved after a review pass,
//! and moving them re-decides stored scores without sending text to Jev again.

use serde::Serialize;

use super::{ChoiceOption, Question, QuestionAction, QuestionKind};

/// A named group of default questions, as the Add menu offers it.
#[derive(Clone, Debug, Serialize)]
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    pub detail: &'static str,
    pub questions: Vec<Question>,
}

/// Every default, in the order the file is seeded.
pub fn default_questions() -> Vec<Question> {
    presets().into_iter().flat_map(|p| p.questions).collect()
}

/// The defaults, grouped by the axis each answers.
pub fn presets() -> Vec<Preset> {
    vec![
        Preset {
            id: "junk",
            label: "Junk gate",
            detail: "One choice: record, service information, or junk. Junk kinds are \
                     excluded when together they reach 0.90. From 0.50 to 0.90 is held for \
                     review.",
            questions: vec![junk_gate()],
        },
        Preset {
            id: "record_type",
            label: "Record type",
            detail: "One choice for the form of the record: agenda, minutes, law, budget, \
                     notice, report, data table, and more. Each kind is a tag.",
            questions: vec![record_type()],
        },
        Preset {
            id: "topics",
            label: "Topics",
            detail: "Yes-or-no tags for what the text is about: laws, land, money, \
                     transportation, water, housing, safety, taxes, procurement, people, \
                     courts, health, grants. One document can have several.",
            questions: topics(),
        },
        Preset {
            id: "body",
            label: "Who decided",
            detail: "One choice for the body whose act the text records: county board, city \
                     council, advisory board, constitutional officer, staff, regional \
                     agency, nonprofit.",
            questions: vec![body()],
        },
        Preset {
            id: "stage",
            label: "Decision stage",
            detail: "One choice for where the text sits in a decision: proposed, adopted, in \
                     effect, reported after the fact, or informational.",
            questions: vec![decision_stage()],
        },
        Preset {
            id: "audience",
            label: "Who it touches",
            detail: "Yes-or-no tags for who the text affects: property owners, businesses, \
                     residents at large, the government itself.",
            questions: audience(),
        },
        Preset {
            id: "action",
            label: "Actionability",
            detail: "Yes-or-no tags for a deadline to act by, and for an outside \
                     organization named in the text.",
            questions: actionability(),
        },
    ]
}

fn option(id: &str, action: QuestionAction, description: &str) -> ChoiceOption {
    ChoiceOption {
        id: id.into(),
        description: description.into(),
        action,
    }
}

fn choice(id: &str, threshold: f64, instructions: &str, options: Vec<ChoiceOption>) -> Question {
    Question {
        id: id.into(),
        instructions: instructions.into(),
        version: 0,
        kind: QuestionKind::Choice,
        options,
        threshold,
        review: Some(0.5),
        action: QuestionAction::Tag,
        when: None,
    }
}

fn noul(id: &str, instructions: &str) -> Question {
    Question {
        id: id.into(),
        instructions: instructions.into(),
        version: 0,
        kind: QuestionKind::Noul,
        options: Vec::new(),
        threshold: 0.8,
        review: Some(0.5),
        action: QuestionAction::Tag,
        when: None,
    }
}

/// Is it junk? One choice. The exclude options are summed, so "half navigation, half
/// calendar" is still junk.
fn junk_gate() -> Question {
    use QuestionAction::{Exclude, Keep};
    choice(
        "page_kind",
        0.9,
        "What is this city government text mainly? Judge the main content, not the menus, \
         headers, and footers around it. Option definitions take precedence over page titles \
         and site headers.",
        vec![
            option(
                "record",
                Keep,
                "A substantive government record or statement with content of its own: an agenda, minutes, ordinance, resolution, code section, budget or financial report, contract, plan, study, staff report, presentation, or a notice that names a specific decision, hearing, or date. It belongs here even when site menus surround it. Attachments and exhibits of a record belong here.",
            ),
            option(
                "service_info",
                Keep,
                "Standing information about a city service or program with substantive text of its own: what it is, who qualifies, how to apply, fees, rules, or requirements. Not a list of links to such pages.",
            ),
            option(
                "navigation",
                Exclude,
                "A page whose main content is menus, link lists, search results, or tiles that point to other pages, with little text of its own. Signals: many short link labels such as \"Home\", \"Quick Links\", \"Contact Us\", \"Site Map\". Not a page that summarizes one named record.",
            ),
            option(
                "calendar_or_directory",
                Exclude,
                "A listing with no decision or record content: an event calendar, a list of meeting dates, a staff or phone directory, office hours, a holiday schedule, a trash or recycling pickup schedule, or a map legend. Not an agenda, which lists items for decision.",
            ),
            option(
                "unreadable",
                Exclude,
                "Text that cannot be used: empty or garbled extraction, character noise, only a print notice or a note that the document is available elsewhere, an error page, or a cookie or login wall.",
            ),
            option(
                "other",
                Keep,
                "The main content fits none of the other options.",
            ),
        ],
    )
}

/// What form the record takes. One choice with a tag for each kind.
fn record_type() -> Question {
    use QuestionAction::{Keep, Tag};
    choice(
        "record_type",
        0.75,
        "What kind of government record is this text? Choose by the purpose of the whole \
         document, not by words it mentions. Attachments belong to the record they are \
         attached to.",
        vec![
            option(
                "agenda",
                Tag,
                "An agenda for a meeting: a numbered list of items to be considered. Signals: \"CALL TO ORDER\", \"CONSENT AGENDA\", \"PUBLIC COMMENT\". Not minutes of a meeting already held.",
            ),
            option(
                "minutes",
                Tag,
                "A record of a meeting already held. Signals: \"Present:\", \"Motion by\", \"seconded\", \"carried\". Not an agenda.",
            ),
            option(
                "law_text",
                Tag,
                "The text of an ordinance or resolution, or a code or charter section. Signals: \"AN ORDINANCE OF\", \"BE IT ORDAINED\", \"WHEREAS\", \"NOW, THEREFORE, BE IT RESOLVED\".",
            ),
            option(
                "budget_document",
                Tag,
                "A budget, financial statement, audit, or financial report with tables of amounts and narrative around them. Not a bare export of rows.",
            ),
            option(
                "public_notice",
                Tag,
                "A notice to the public of a hearing, meeting, comment period, bid, or decision, with a date or deadline.",
            ),
            option(
                "staff_report",
                Tag,
                "A staff report, memo, or agenda item summary that analyzes one item and makes a recommendation.",
            ),
            option(
                "plan_or_study",
                Tag,
                "A plan, study, assessment, or report on a topic, such as a comprehensive plan or a traffic study.",
            ),
            option(
                "contract_or_procurement",
                Tag,
                "A contract, agreement, bid, request for proposals, or award.",
            ),
            option(
                "application_or_form",
                Tag,
                "An application, permit form, or checklist to fill in.",
            ),
            option(
                "data_table",
                Tag,
                "Rows and columns of values with no narrative around them: a register, a ledger or payment export, a list of permits, cases, parcels, or payments. Signals: repeated delimiters, column headers, one record per line. Not a budget document, which has narrative and totals.",
            ),
            option("other", Keep, "None of the other options fits."),
        ],
    )
}

/// What the text is about. Yes-or-no, because one document can be about several.
fn topics() -> Vec<Question> {
    vec![
        noul(
            "laws",
            "Does `text` contain the substance of a city or county law or legal rule: an ordinance, resolution, code or charter section, or the text or summary of a proposed or adopted change to one? Naming a law without its content is not enough.",
        ),
        noul(
            "real_estate",
            "Does `text` contain substantive information about land, property, or development: zoning, rezoning, land use, variances, site plans, building or development permits, property sales, leases, acquisitions, easements, or property values and assessments?",
        ),
        noul(
            "budget",
            "Does `text` contain substantive information about public money: a budget, appropriation, revenue, taxes, fees, spending, debt, audit, financial report, or grant, with amounts or decisions?",
        ),
        noul(
            "policy",
            "Does `text` state a government policy, plan, or program decision: what the government decided or proposes to do, and why or how? Examples are strategic or comprehensive plans, policy statements, program rules, and staff recommendations. Instructions for using a service are not enough.",
        ),
        noul(
            "public_participation",
            "Does `text` announce a current or upcoming opportunity for the public to give input into a government decision, such as a public hearing, comment period, or survey, with a date or a way to take part? A report that the public already gave input is not enough.",
        ),
        noul(
            "transportation",
            "Does `text` contain substantive information about transportation: roads, transit, buses or rail, sidewalks, bicycle facilities, parking, traffic, bridges, ports, or airports, with a project, rule, decision, or data?",
        ),
        noul(
            "water_environment",
            "Does `text` contain substantive information about water or the environment: stormwater, flooding, drainage, water supply, sewer or wastewater, wetlands, coasts, climate or sea level, pollution, conservation, or parks as natural land?",
        ),
        noul(
            "housing",
            "Does `text` contain substantive information about housing: affordable housing, homelessness, rental assistance, housing programs, housing construction, or tenant and landlord rules?",
        ),
        noul(
            "public_safety",
            "Does `text` contain substantive information about public safety: police, fire, emergency medical services, emergency management, disasters, code enforcement, or crime, with a decision, rule, program, or data?",
        ),
        noul(
            "taxes_fees",
            "Does `text` contain substantive information about taxes or fees: tax rates, millage, assessments, exemptions, fee schedules, tax collection, or payment rules, with amounts or decisions?",
        ),
        noul(
            "procurement",
            "Does `text` concern a purchase or contract with an outside party: a bid, request for proposals, award, vendor, contract, change order, or payment to a contractor or supplier?",
        ),
        noul(
            "personnel",
            "Does `text` concern the government's own workforce: hiring, pay, benefits, pensions, unions, staffing, discipline, or an appointment to a position or board?",
        ),
        noul(
            "courts_records",
            "Does `text` concern court or official-record functions: court filings or cases, official records, recording of documents, marriage or business licenses, jury service, or the clerk's duties?",
        ),
        noul(
            "health_human_services",
            "Does `text` contain substantive information about health or human services: public health, aging, children and families, disability, veterans, social or financial assistance, or community programs for people in need?",
        ),
        noul(
            "grants_philanthropy",
            "Does `text` concern a grant or gift: a grant made, sought, or awarded, a donor or fund, a foundation or nonprofit program, or how to apply for funding?",
        ),
    ]
}

/// Whose act the text records. One choice, because a record has one author.
fn body() -> Question {
    use QuestionAction::{Keep, Tag};
    choice(
        "body",
        0.75,
        "Which body's act or voice does this text mainly record? Choose the body that \
         decided, published, or is speaking, not every body the text mentions.",
        vec![
            option(
                "county_commission",
                Tag,
                "The elected county board: its agendas, minutes, resolutions, ordinances, and actions. Signals: \"Board of County Commissioners\", \"BOCC\", commissioners by name. Not a city council.",
            ),
            option(
                "city_council",
                Tag,
                "A city council or city commission and the mayor's office. Signals: \"City Council\", \"Council Member\", \"Mayor\". Not a county board.",
            ),
            option(
                "advisory_board",
                Tag,
                "An appointed board, commission, authority, or committee: planning commission, zoning or variance board, architectural review, community redevelopment agency, metropolitan planning organization, code enforcement board, citizens advisory committee. Not the elected board it advises.",
            ),
            option(
                "constitutional_officer",
                Tag,
                "An elected officer's own office: clerk of court, tax collector, property appraiser, sheriff, supervisor of elections, state attorney, public defender.",
            ),
            option(
                "department_staff",
                Tag,
                "A government department or office acting for itself: public works, utilities, parks, planning staff, human resources, a program office. Signals: a department name, staff titles, no vote recorded.",
            ),
            option(
                "regional_agency",
                Tag,
                "A regional or multi-jurisdiction body: regional planning council, transit authority, water management district, port or aviation authority, school district.",
            ),
            option(
                "nonprofit",
                Tag,
                "A nonprofit, foundation, or community organization speaking for itself: a grant maker, a civic group, a charity.",
            ),
            option(
                "other",
                Keep,
                "None of the other options fits, or no body can be told from the text.",
            ),
        ],
    )
}

/// Where the text sits in a decision. Minutes of a past hearing stop looking like future
/// hearings, which was the lesson of the September trial.
fn decision_stage() -> Question {
    use QuestionAction::{Keep, Tag};
    choice(
        "decision_stage",
        0.75,
        "Where does this text sit in a government decision? Judge the whole document: what \
         it mainly does, not every item it mentions.",
        vec![
            option(
                "proposed",
                Tag,
                "Something not yet decided: an item on an upcoming agenda, a draft, a proposed ordinance, an application under review, a scheduled hearing. Signals: \"proposed\", \"first reading\", \"draft\", \"will consider\", a future date.",
            ),
            option(
                "adopted",
                Tag,
                "A decision made: passed, approved, adopted, awarded, denied, enacted. Signals: \"adopted\", \"approved\", \"motion carried\", \"awarded to\", an effective date.",
            ),
            option(
                "in_effect",
                Tag,
                "A standing rule or program as it operates now: a code section, fee schedule, policy, or how a service works, with no decision event in the text.",
            ),
            option(
                "reported",
                Tag,
                "An account of something that already happened: minutes, a report of results, an audit finding, attendance, a summary of public comment received.",
            ),
            option(
                "informational",
                Tag,
                "No decision and no rule: general information, news, a description, an announcement without a decision in it.",
            ),
            option("other", Keep, "None of the other options fits."),
        ],
    )
}

/// Who the text touches. Yes-or-no, because a rezoning hits owners and neighbours alike.
fn audience() -> Vec<Question> {
    vec![
        noul(
            "affects_property_owners",
            "Does `text` affect property owners or a specific parcel, street, or neighbourhood: zoning, assessments, code enforcement, permits, utilities to a property, or a project on or beside named land?",
        ),
        noul(
            "affects_businesses",
            "Does `text` affect businesses or contractors: licensing, business taxes or fees, procurement, vendor rules, commercial zoning, or regulation of a trade?",
        ),
        noul(
            "affects_residents",
            "Does `text` affect residents at large: a public service, a program anyone can use, a rule that applies to everyone in the jurisdiction, a utility rate, or a public facility?",
        ),
        noul(
            "affects_government_internal",
            "Is `text` mainly about the government's own operation: its staff, internal procedures, administration, its own finances, or an appointment, rather than something the public does or receives?",
        ),
    ]
}

/// Whether a reader can act on the text, and whether it points outside the government.
fn actionability() -> Vec<Question> {
    vec![
        noul(
            "has_deadline",
            "Does `text` name a date or deadline by which the public or a party must act: a comment deadline, an application due date, a bid closing, a hearing at which to appear, a payment or filing deadline? A meeting date alone is not enough unless the public is asked to act by it.",
        ),
        noul(
            "names_outside_org",
            "Does `text` name a specific organization outside this government by its own name: a company, contractor, vendor, nonprofit, foundation, authority, university, or another government? A generic mention such as \"the contractor\" is not enough.",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::validate_questions;

    /// Every default would be accepted by `save_questions`, and every choice has the
    /// escape option the module doc insists on.
    #[test]
    fn defaults_are_valid_and_every_choice_has_an_escape() {
        let questions = default_questions();
        validate_questions(&questions).unwrap();
        for question in &questions {
            if question.kind == QuestionKind::Choice {
                assert!(
                    question.options.iter().any(|o| o.id == "other"),
                    "choice `{}` has no `other` option",
                    question.id
                );
            }
            assert!(
                question.instructions.contains("`text`") || question.kind == QuestionKind::Choice,
                "noul `{}` does not say what it is judging",
                question.id
            );
        }
        let ids: std::collections::HashSet<_> = questions.iter().map(|q| q.id.as_str()).collect();
        assert_eq!(ids.len(), questions.len(), "ids repeat across presets");
    }

    /// The gate excludes exactly the junk kinds and keeps the rest; no other default
    /// excludes anything, because a tag is not a usage decision.
    #[test]
    fn only_the_gate_excludes_and_only_its_junk_kinds() {
        let gate = junk_gate();
        let excluded: Vec<&str> = gate
            .options
            .iter()
            .filter(|o| o.action == QuestionAction::Exclude)
            .map(|o| o.id.as_str())
            .collect();
        assert_eq!(
            excluded,
            ["navigation", "calendar_or_directory", "unreadable"]
        );
        for question in default_questions().iter().filter(|q| q.id != "page_kind") {
            assert_ne!(question.action, QuestionAction::Exclude, "{}", question.id);
            assert!(
                question
                    .options
                    .iter()
                    .all(|o| o.action != QuestionAction::Exclude),
                "{} has an exclude option",
                question.id
            );
        }
    }

    /// The publicrec case: 135 CSVs became half the index, and nothing in the gate
    /// described a table of rows. The record type names it, so search can ask for it or
    /// leave it out.
    #[test]
    fn a_data_table_is_a_record_kind() {
        let kinds = record_type();
        let table = kinds
            .options
            .iter()
            .find(|o| o.id == "data_table")
            .expect("data_table option");
        assert_eq!(table.action, QuestionAction::Tag);
    }

    #[test]
    fn presets_flatten_to_the_default_set_in_order() {
        let flat = default_questions();
        let from_presets: Vec<String> = presets()
            .into_iter()
            .flat_map(|p| p.questions)
            .map(|q| q.id)
            .collect();
        assert_eq!(
            flat.iter().map(|q| q.id.clone()).collect::<Vec<_>>(),
            from_presets
        );
        assert_eq!(flat[0].id, "page_kind", "the gate is seeded first");
    }
}
