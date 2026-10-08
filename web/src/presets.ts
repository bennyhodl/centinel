import type { ChoiceOption, Question, QuestionAction } from './api'

/**
 * Starting questions for a city-government corpus. Each is only a draft until it is
 * saved; after that its wording is versioned meaning and its thresholds are policy.
 *
 * How they are written follows what held up in other Jev classifiers:
 * - A choice for "what kind of thing is this", with the junk kinds beside the useful
 *   kinds, so that junk competes with records for the same probability.
 * - An `other` option in every choice, so a document that fits nothing is not forced
 *   into a wrong option.
 * - Options defined by purpose, with the words a reader would see on the page, and
 *   with the one or two options each is most likely confused with.
 * - Yes-or-no questions for topics, because one document can be about several.
 */

const option = (id: string, action: QuestionAction, description: string): ChoiceOption => ({ id, action, description })

/** Is it junk? One choice. The exclude options are summed, so "half navigation, half calendar" is still junk. */
export const junkGate: Question = {
  id: 'page_kind',
  kind: 'choice',
  version: 0,
  threshold: 0.9,
  review: 0.5,
  action: 'tag',
  instructions: 'What is this city government text mainly? Judge the main content, not the menus, headers, and footers around it. Option definitions take precedence over page titles and site headers.',
  options: [
    option('record', 'keep', 'A substantive government record or statement with content of its own: an agenda, minutes, ordinance, resolution, code section, budget or financial report, contract, plan, study, staff report, presentation, or a notice that names a specific decision, hearing, or date. It belongs here even when site menus surround it. Attachments and exhibits of a record belong here.'),
    option('service_info', 'keep', 'Standing information about a city service or program with substantive text of its own: what it is, who qualifies, how to apply, fees, rules, or requirements. Not a list of links to such pages.'),
    option('navigation', 'exclude', 'A page whose main content is menus, link lists, search results, or tiles that point to other pages, with little text of its own. Signals: many short link labels such as "Home", "Quick Links", "Contact Us", "Site Map". Not a page that summarizes one named record.'),
    option('calendar_or_directory', 'exclude', 'A listing with no decision or record content: an event calendar, a list of meeting dates, a staff or phone directory, office hours, a holiday schedule, a trash or recycling pickup schedule, or a map legend. Not an agenda, which lists items for decision.'),
    option('unreadable', 'exclude', 'Text that cannot be used: empty or garbled extraction, character noise, only a print notice or a note that the document is available elsewhere, an error page, or a cookie or login wall.'),
    option('other', 'keep', 'The main content fits none of the other options.'),
  ],
}

const topic = (id: string, instructions: string): Question => ({
  id, kind: 'noul', version: 0, threshold: 0.8, review: 0.5, action: 'tag', instructions,
})

/** City topics. Yes-or-no tags, because one document can be about several. */
export const cityTopics: Question[] = [
  topic('laws', 'Does `text` contain the substance of a city law or legal rule: an ordinance, resolution, code or charter section, or the text or summary of a proposed or adopted change to one? Naming a law without its content is not enough.'),
  topic('real_estate', 'Does `text` contain substantive information about land, property, or development: zoning, rezoning, land use, variances, site plans, building or development permits, property sales, leases, acquisitions, easements, or property values and assessments?'),
  topic('budget', 'Does `text` contain substantive information about public money: a budget, appropriation, revenue, taxes, fees, spending, debt, audit, financial report, or grant, with amounts or decisions?'),
  topic('policy', 'Does `text` state a government policy, plan, or program decision: what the government decided or proposes to do, and why or how? Examples are strategic or comprehensive plans, policy statements, program rules, and staff recommendations. Instructions for using a service are not enough.'),
  topic('public_participation', 'Does `text` announce a current or upcoming opportunity for the public to give input into a government decision, such as a public hearing, comment period, or survey, with a date or a way to take part? A report that the public already gave input is not enough.'),
]

/** What kind of record is it? One choice with a tag for each kind. */
export const recordType: Question = {
  id: 'record_type',
  kind: 'choice',
  version: 0,
  threshold: 0.75,
  review: 0.5,
  action: 'tag',
  instructions: 'What kind of government record is this text? Choose by the purpose of the whole document, not by words it mentions. Attachments belong to the record they are attached to.',
  options: [
    option('agenda', 'tag', 'An agenda for a meeting: a numbered list of items to be considered. Signals: "CALL TO ORDER", "CONSENT AGENDA", "PUBLIC COMMENT". Not minutes of a meeting already held.'),
    option('minutes', 'tag', 'A record of a meeting already held. Signals: "Present:", "Motion by", "seconded", "carried". Not an agenda.'),
    option('law_text', 'tag', 'The text of an ordinance or resolution, or a code or charter section. Signals: "AN ORDINANCE OF", "BE IT ORDAINED", "WHEREAS", "NOW, THEREFORE, BE IT RESOLVED".'),
    option('budget_document', 'tag', 'A budget, financial statement, audit, or financial report with tables of amounts.'),
    option('public_notice', 'tag', 'A notice to the public of a hearing, meeting, comment period, bid, or decision, with a date or deadline.'),
    option('staff_report', 'tag', 'A staff report, memo, or agenda item summary that analyzes one item and makes a recommendation.'),
    option('plan_or_study', 'tag', 'A plan, study, assessment, or report on a topic, such as a comprehensive plan or a traffic study.'),
    option('contract_or_procurement', 'tag', 'A contract, agreement, bid, request for proposals, or award.'),
    option('application_or_form', 'tag', 'An application, permit form, or checklist to fill in.'),
    option('other', 'keep', 'None of the other options fits.'),
  ],
}

export type Preset = { id: string; label: string; detail: string; questions: Question[] }

export const presets: Preset[] = [
  { id: 'junk', label: 'Junk gate', detail: 'One choice: record, service information, or junk. Junk kinds are excluded when together they reach 0.90. From 0.50 to 0.90 is held for review.', questions: [junkGate] },
  { id: 'topics', label: 'City topics', detail: 'Yes-or-no tags for laws, real estate, budget, policy, and public participation. One document can have several.', questions: cityTopics },
  { id: 'record_type', label: 'Record type', detail: 'One choice for the kind of record: agenda, minutes, law, budget, notice, report, and more.', questions: [recordType] },
]
