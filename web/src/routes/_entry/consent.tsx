import { createFileRoute, redirect } from '@tanstack/react-router'
import { useMutation } from '@tanstack/react-query'
import { Check } from 'lucide-react'
import { answerConsent, oauthRequest } from '../../auth'
import { EntryHeading } from '../../entry'
import { queries } from '../../queries'
import { Button } from '@/components/ui/button'
import { Field, FieldError, FieldGroup } from '@/components/ui/field'
import { Skeleton } from '@/components/ui/skeleton'

export const Route = createFileRoute('/_entry/consent')({
  // Better Auth sends a signed-in person here with the client's request in the query. Anyone
  // else signs in first; the sign-in page keeps the request and picks it up afterwards.
  beforeLoad: async ({ context: { queryClient }, location }) => {
    const account = await queryClient.ensureQueryData(queries.account())
    if (account?.access !== 'approved') throw redirect({ href: `/web/login${location.searchStr}` })
    const request = oauthRequest(location.searchStr)
    if (!request) throw redirect({ to: '/', search: {} })
    return { request, account }
  },
  pendingComponent: ConsentPending,
  component: Consent,
})

/** What each OAuth scope lets the client do, in the person's words. */
const permissions: Record<string, string> = {
  'mcp:tools': 'Search, read and download from this archive',
  openid: 'Know which account it acts as',
  profile: 'See your name and picture',
  offline_access: 'Stay connected without asking again',
}

/** While the account is read: the request's frame, with the client and its asks blank. */
function ConsentPending() {
  return <FieldGroup aria-busy>
    <EntryHeading title="Connect an agent">
      <p className="text-balance text-sm"><Skeleton mask="An agent wants to use Centinel as Archive reader. It can do no more than you can." /></p>
    </EntryHeading>
    <Skeleton className="h-40 rounded-lg" />
    <Field><Skeleton className="h-9 rounded-md" /><Skeleton className="h-9 rounded-md" /></Field>
  </FieldGroup>
}

function Consent() {
  const { request, account } = Route.useRouteContext()
  const answer = useMutation({ mutationFn: (accept: boolean) => answerConsent(request, accept) })
  const scopes = request.get('scope')?.split(' ').filter(Boolean) ?? []
  return <FieldGroup>
    <EntryHeading title="Connect an agent">
      <p className="text-balance text-sm text-muted-foreground">An agent wants to use Centinel as {account.name}. It can do no more than you can.</p>
    </EntryHeading>
    <div className="rounded-lg border">
      <div className="border-b px-4 py-3">
        <p className="text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">Client</p>
        <p className="mt-0.5 truncate font-mono text-xs" title={request.get('client_id') ?? ''}>{request.get('client_id')}</p>
      </div>
      <ul className="grid gap-2.5 px-4 py-3 text-sm">
        {scopes.map(scope => <li key={scope} className="flex items-start gap-2.5"><Check className="mt-0.5 size-4 shrink-0 text-moss" />{permissions[scope] ?? scope}</li>)}
      </ul>
    </div>
    <Field>
      <Button disabled={answer.isPending} onClick={() => answer.mutate(true)}>Allow</Button>
      <Button variant="outline" disabled={answer.isPending} onClick={() => answer.mutate(false)}>Deny</Button>
      <FieldError>{answer.error?.message}</FieldError>
    </Field>
  </FieldGroup>
}
