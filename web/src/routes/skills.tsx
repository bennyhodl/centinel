import { createFileRoute, Link } from '@tanstack/react-router'
import { Search } from 'lucide-react'
import { PageHeader, SectionRule } from '../ui'
import { CopyBlock } from '../copy'

export const Route = createFileRoute('/skills')({ component: Skills })

/** The skills `npx skills add bennyhodl/centinel` installs. One entry per directory in contrib/skills. */
const skills = [
  {
    name: 'Sherlock',
    invoke: '/sherlock <place>',
    about: 'Name a city, county or region. Sherlock finds the NGOs, grant makers, quasi-public bodies and contract vendors tied to its government that no page in the corpus links to, and writes them to prospects/<place>.json as tips for you to promote.',
    not: 'Not for judging a host that is already a source. That is `centinel investigate`.',
  },
]

function Skills() {
  return <>
    <PageHeader title="Skills" detail="Skills teach your agent a Centinel job end to end. Add them once, and your agent uses them when the job comes up." />
    <div className="grid max-w-3xl gap-10">
      <section className="grid gap-3">
        <SectionRule aside="Claude Code, Cursor, Codex and others">Add every Centinel skill</SectionRule>
        <CopyBlock text="npx skills add bennyhodl/centinel" shell />
        <p className="text-[13px] text-muted-foreground">Skills call Centinel through MCP, so <Link to="/connect" className="underline">connect the server</Link> first.</p>
      </section>
      <section className="grid">
        <SectionRule>{`Available · ${skills.length}`}</SectionRule>
        {skills.map(skill => <div key={skill.name} className="flex items-start gap-4 border-b py-5">
          <span className="grid size-11 shrink-0 place-items-center rounded-[10px] bg-foreground text-parchment"><Search className="size-5" /></span>
          <div className="grid gap-1.5">
            <div className="flex items-baseline gap-2.5"><span className="font-serif text-2xl leading-[26px]">{skill.name}</span><code className="font-mono text-xs text-muted-foreground">{skill.invoke}</code></div>
            <p className="text-sm leading-[21px] text-[#3A352D]">{skill.about}</p>
            <p className="text-xs text-muted-foreground">{skill.not}</p>
          </div>
        </div>)}
        <p className="py-4 text-sm text-muted-foreground">New skills land in contrib/skills/. Run the same command again to pick them up.</p>
      </section>
    </div>
  </>
}
