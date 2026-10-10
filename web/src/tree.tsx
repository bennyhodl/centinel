import { useEffect, useMemo, type ReactNode } from 'react'
import { Background, BackgroundVariant, Controls, Handle, Position, ReactFlow, useReactFlow, type Node, type BuiltInEdge, type NodeProps } from '@xyflow/react'
import dagre from '@dagrejs/dagre'
import '@xyflow/react/dist/style.css'
import { Pencil, Plus, Trash2 } from 'lucide-react'
import type { Question } from './api'
import { useTheme } from './theme'
import { answered, isChoice, outcomesOf, policyShort, probabilityOf, questionProblem } from './policy'

/** A question as the page edits it: a stable key, and whether the next run asks it. */
export type TreeQuestion = Question & { localKey: string; skip?: boolean }

const tagsOf = (question: Question) => outcomesOf(question).flatMap(branch => branch.tag ? [branch.tag] : [])

/** Questions no answer leads to. Each one starts a chain. */
export function sourcesOf<Q extends TreeQuestion>(questions: Q[]): Q[] {
  const tags = new Set(questions.flatMap(tagsOf))
  return questions.filter(question => !question.when || !tags.has(question.when))
}

/** A source and every follow-up below it, top to bottom. */
export function chainOf<Q extends TreeQuestion>(source: Q, questions: Q[]): Q[] {
  const chain = [source]
  for (let i = 0; i < chain.length; i++) {
    for (const tag of tagsOf(chain[i])) chain.push(...questions.filter(question => question.when === tag && !chain.includes(question)))
  }
  return chain
}

type CardData = {
  question: TreeQuestion
  source: boolean
  dirty: boolean
  /** Whether the document under test would reach this question. */
  reached: boolean
  testing: boolean
  answers?: Record<string, number>
  onEdit: (localKey: string) => void
  onDelete: (localKey: string) => void
  onFollow: (tag: string) => void
  onToggle: (localKey: string, run: boolean) => void
}
type CardNode = Node<CardData, 'question'>

const CARD_WIDTH = 320
/** Card height from its parts, so the waterfall lays out before anything is measured. */
const cardHeight = (question: Question) => {
  const lines = Math.max(1, Math.ceil((question.instructions || 'Write the question').length / 36))
  return 94 + lines * 21 + Math.ceil(outcomesOf(question).length / 2) * 40
}

function layout(chain: TreeQuestion[], data: Omit<CardData, 'question' | 'source' | 'dirty' | 'reached'> & { isDirty: (question: TreeQuestion) => boolean }) {
  const graph = new dagre.graphlib.Graph()
  graph.setGraph({ rankdir: 'TB', nodesep: 56, ranksep: 84 })
  graph.setDefaultEdgeLabel(() => ({}))
  const reached = new Map<string, boolean>([[chain[0].localKey, true]])
  const edges: BuiltInEdge[] = []
  for (const question of chain) {
    graph.setNode(question.localKey, { width: CARD_WIDTH, height: cardHeight(question) })
    const lit = reached.get(question.localKey) ? answered(question, data.answers) : undefined
    for (const branch of outcomesOf(question)) {
      if (!branch.tag) continue
      for (const child of chain.filter(c => c.when === branch.tag)) {
        const on = lit === branch.label
        reached.set(child.localKey, Boolean(reached.get(question.localKey) && on))
        graph.setEdge(question.localKey, child.localKey)
        edges.push({
          id: `${question.localKey}>${child.localKey}`, source: question.localKey, sourceHandle: branch.tag, target: child.localKey, type: 'smoothstep',
          pathOptions: { borderRadius: 14 },
          style: on ? { stroke: 'var(--flame)', strokeWidth: 2.5 } : data.testing ? { stroke: 'var(--input)', strokeWidth: 1.5, strokeDasharray: '5 5' } : { stroke: 'var(--stone)', strokeWidth: 1.5 },
        })
      }
    }
  }
  dagre.layout(graph)
  const nodes: CardNode[] = chain.map((question, index) => {
    const { x, y, width, height } = graph.node(question.localKey)
    return {
      id: question.localKey, type: 'question', position: { x: x - width / 2, y: y - height / 2 },
      data: { ...data, question, source: index === 0, dirty: data.isDirty(question), reached: reached.get(question.localKey) ?? false },
    }
  })
  return { nodes, edges }
}

const nodeTypes = { question: QuestionCard }

/** One chain on a canvas you can pan and zoom: the source on top, follow-ups falling below the answer they follow. */
export function QuestionFlow({ chain, isDirty, answers, onEdit, onDelete, onFollow, onToggle, children }: Omit<CardData, 'question' | 'source' | 'dirty' | 'reached' | 'testing'> & { chain: TreeQuestion[]; isDirty: (question: TreeQuestion) => boolean; children?: ReactNode }) {
  const testing = Boolean(answers)
  const [theme] = useTheme()
  const { nodes, edges } = useMemo(
    () => chain.length ? layout(chain, { isDirty, answers, testing, onEdit, onDelete, onFollow, onToggle }) : { nodes: [], edges: [] },
    [chain, isDirty, answers, testing, onEdit, onDelete, onFollow, onToggle],
  )
  return <ReactFlow colorMode={theme} nodes={nodes} edges={edges} nodeTypes={nodeTypes} fitView fitViewOptions={{ padding: 0.2, maxZoom: 1 }} minZoom={0.15} maxZoom={1.75}
    nodesDraggable={false} nodesConnectable={false} elementsSelectable={false} onNodeClick={(_, node) => onEdit(node.id)} proOptions={{ hideAttribution: true }}>
    <Background variant={BackgroundVariant.Dots} gap={18} size={1.4} color="var(--dot)" bgColor="var(--canvas)" />
    <Controls showInteractive={false} position="bottom-left" />
    <Refit signature={chain.map(question => question.localKey).join()} />
    {children}
  </ReactFlow>
}

/** Frame the chain again when its shape changes: another source, a question added or removed. */
function Refit({ signature }: { signature: string }) {
  const { fitView } = useReactFlow()
  useEffect(() => { const frame = requestAnimationFrame(() => fitView({ padding: 0.2, maxZoom: 1, duration: 250 })); return () => cancelAnimationFrame(frame) }, [signature, fitView])
  return null
}

const actionWord = { exclude: 'exclude', tag: 'tag', keep: 'keep' } as const

function QuestionCard({ data }: NodeProps<CardNode>) {
  const { question, source, dirty, reached, testing, answers } = data
  const problem = questionProblem(question)
  const lit = reached ? answered(question, answers) : undefined
  const dim = testing && !reached
  return <div className={`w-[320px] cursor-pointer rounded-xl bg-background ${testing && reached ? 'shadow-[0_0_0_1.5px_var(--flame),0_6px_18px_rgba(26,23,18,0.10)]' : 'shadow-[0_0_0_1.5px_var(--foreground),0_6px_18px_rgba(26,23,18,0.08)]'} ${dim || question.skip ? 'opacity-50' : ''}`}>
    {!source && <Handle type="target" position={Position.Top} isConnectable={false} className="!size-2 !min-h-0 !min-w-0 !border-0 !bg-stone" />}
    <div className="flex items-center gap-2 px-4 pt-3">
      <input type="checkbox" checked={!question.skip} onClick={event => event.stopPropagation()} onChange={event => data.onToggle(question.localKey, event.target.checked)} aria-label={`Run ${question.id}`} className="nodrag accent-foreground" />
      <span className={`shrink-0 text-[10px] font-bold tracking-[0.1em] ${problem ? 'text-destructive' : dirty ? 'text-flame-ink' : 'text-muted-foreground'}`}>{source ? 'SOURCE · ' : ''}{isChoice(question) ? 'CHOICE' : 'NOUL'} · {problem ? 'fix' : dirty ? 'unsaved' : `v${question.version}`}</span>
      <span className="min-w-0 flex-1 truncate text-right font-mono text-[11px] text-muted-foreground">{question.id}</span>
      <button type="button" aria-label={`Edit ${question.id}`} onClick={event => { event.stopPropagation(); data.onEdit(question.localKey) }} className="nodrag grid size-6 place-items-center rounded-md text-muted-foreground hover:bg-parchment hover:text-foreground"><Pencil className="size-3.5" /></button>
      <button type="button" aria-label={`Delete ${question.id}`} onClick={event => { event.stopPropagation(); data.onDelete(question.localKey) }} className="nodrag grid size-6 place-items-center rounded-md text-muted-foreground hover:bg-destructive-soft hover:text-destructive"><Trash2 className="size-3.5" /></button>
    </div>
    <p className={`px-4 pt-1.5 text-[15px] leading-[21px] font-semibold whitespace-pre-wrap ${question.instructions ? '' : 'text-muted-foreground'}`}>{question.instructions || 'Write the question'}</p>
    <p className="px-4 pt-1 text-xs text-muted-foreground">{policyShort(question)}</p>
    <div className="mt-3 grid grid-cols-2 gap-1.5 border-t p-3">
      {outcomesOf(question).map(branch => {
        const on = lit === branch.label
        const p = answers && reached ? probabilityOf(question, branch, answers) : undefined
        return <div key={branch.label} className={`group/chip relative flex h-[34px] min-w-0 items-center gap-1.5 rounded-lg px-2.5 text-[13px] ${on ? 'bg-flame-soft font-bold shadow-[0_0_0_1.5px_var(--flame)]' : 'bg-canvas shadow-[0_0_0_1px_var(--rule)]'}`}>
          <span className="min-w-0 flex-1 truncate" title={branch.label}>{branch.label}</span>
          {p != null ? <span className={`font-mono text-xs ${on ? 'text-flame-ink' : 'text-muted-foreground'}`}>{Math.round(p * 100)}%</span> : <span className={`text-[10px] ${branch.action === 'exclude' ? 'text-destructive' : 'text-muted-foreground'}`}>{actionWord[branch.action]}</span>}
          {branch.tag && <Handle type="source" id={branch.tag} position={Position.Bottom} isConnectable={false} className="!size-1.5 !min-h-0 !min-w-0 !border-0 !bg-transparent" />}
          {branch.tag && <button type="button" aria-label={`Ask a follow-up on ${branch.label}`} onClick={event => { event.stopPropagation(); data.onFollow(branch.tag!) }} className="nodrag absolute -bottom-3 left-1/2 z-10 grid size-6 -translate-x-1/2 place-items-center rounded-full border border-input bg-background text-muted-foreground opacity-0 shadow-xs group-hover/chip:opacity-100 hover:text-foreground focus:opacity-100"><Plus className="size-3.5" /></button>}
        </div>
      })}
    </div>
  </div>
}
