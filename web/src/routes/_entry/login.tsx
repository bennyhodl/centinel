import { useState, type ReactNode } from 'react'
import { createFileRoute, Navigate, redirect, useLocation } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { KeyRound } from 'lucide-react'
import type { Account } from '../../api'
import { continueAuthorization, oauthRequest, signIn, signInWithNostr, signOut, signUp } from '../../auth'
import { EntryHeading } from '../../entry'
import { queries } from '../../queries'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel, FieldSeparator } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Skeleton } from '@/components/ui/skeleton'

export const Route = createFileRoute('/_entry/login')({
  // Someone approved has nothing to do here, unless an agent sent them to pick up its authorization.
  beforeLoad: async ({ context: { queryClient }, location }) => {
    const account = await queryClient.ensureQueryData(queries.account())
    if (account?.access === 'approved' && !oauthRequest(location.searchStr)) throw redirect({ to: '/', search: {} })
  },
  pendingComponent: LoginPending,
  component: Login,
})

function Login() {
  const account = useQuery(queries.account()).data
  const request = oauthRequest(useLocation({ select: location => location.searchStr }))
  if (!account) return <SignIn />
  if (account.access !== 'approved') return <Waiting account={account} />
  if (request) return <Continue account={account} request={request} />
  return <Navigate to="/" search={{}} />
}

/**
 * While the account is read: the sign-in form, which is what most people get, with its
 * fields and buttons blank so nothing can be typed into a form about to be replaced.
 */
function LoginPending() {
  return <FieldGroup aria-busy>
    <EntryHeading title="Centinel"><FieldDescription><Skeleton mask="New here? Request access" /></FieldDescription></EntryHeading>
    <Field><FieldLabel>Email</FieldLabel><Skeleton className="h-9 rounded-md" /></Field>
    <Field><FieldLabel>Password</FieldLabel><Skeleton className="h-9 rounded-md" /></Field>
    <Field><Skeleton className="h-9 rounded-md" /></Field>
    <FieldSeparator className="dark:*:data-[slot=field-separator-content]:bg-night">Or</FieldSeparator>
    <Field><Skeleton className="h-9 rounded-md" /></Field>
    <FieldDescription className="px-6 text-center"><Skeleton mask="Nostr signs with your browser extension. Your key stays in it." /></FieldDescription>
  </FieldGroup>
}

function SignIn() {
  const [requesting, setRequesting] = useState(false)
  const queryClient = useQueryClient()
  // Whichever way in, the account is re-read before the form lets go, so the page changes once.
  const enter = useMutation({
    mutationFn: async (way: () => Promise<void>) => { await way(); await queryClient.refetchQueries(queries.account()) },
  })
  const submit = (form: FormData) => {
    const credentials = { email: String(form.get('email')), password: String(form.get('password')) }
    enter.mutate(() => requesting ? signUp({ ...credentials, name: String(form.get('name')) }) : signIn(credentials))
  }
  const switchMode = () => { setRequesting(!requesting); enter.reset() }
  return <form onSubmit={event => { event.preventDefault(); submit(new FormData(event.currentTarget)) }}>
    <FieldGroup>
      <EntryHeading title={requesting ? 'Request access' : 'Centinel'}>
        <FieldDescription>
          {requesting ? 'Have an account? ' : 'New here? '}
          <button type="button" className="underline underline-offset-4 hover:text-primary" onClick={switchMode}>{requesting ? 'Sign in' : 'Request access'}</button>
        </FieldDescription>
      </EntryHeading>
      {requesting && <Field>
        <FieldLabel htmlFor="name">Name</FieldLabel>
        <Input id="name" name="name" autoComplete="name" required maxLength={100} />
      </Field>}
      <Field>
        <FieldLabel htmlFor="email">Email</FieldLabel>
        <Input id="email" name="email" type="email" autoComplete="email" required />
      </Field>
      <Field>
        <FieldLabel htmlFor="password">Password</FieldLabel>
        <Input id="password" name="password" type="password" autoComplete={requesting ? 'new-password' : 'current-password'} required minLength={requesting ? 12 : undefined} />
        {requesting && <FieldDescription>At least 12 characters.</FieldDescription>}
      </Field>
      <Field>
        <Button type="submit" disabled={enter.isPending}>{requesting ? 'Request access' : 'Sign in'}</Button>
        <FieldError>{enter.error?.message}</FieldError>
      </Field>
      <FieldSeparator className="dark:*:data-[slot=field-separator-content]:bg-night">Or</FieldSeparator>
      <Field>
        <Button type="button" variant="outline" disabled={enter.isPending} onClick={() => enter.mutate(signInWithNostr)}><KeyRound />Continue with Nostr</Button>
      </Field>
      <FieldDescription className="px-6 text-center">
        {requesting ? 'The first account becomes the administrator. Every later one waits for approval.' : 'Nostr signs with your browser extension. Your key stays in it.'}
      </FieldDescription>
    </FieldGroup>
  </form>
}

const waiting: Record<'pending' | 'revoked', [string, string]> = {
  pending: ['Awaiting approval', 'Your account is ready. The administrator must approve it before you can search this archive or connect an agent.'],
  revoked: ['Access revoked', 'The administrator has taken back your access. Ask them to approve it again.'],
}

function Waiting({ account }: { account: Account }) {
  const [title, detail] = waiting[account.access === 'revoked' ? 'revoked' : 'pending']
  return <Panel title={title} detail={detail}>
    <FieldDescription className="text-center">Signed in as {account.name}.</FieldDescription>
    <Field><Button variant="outline" onClick={signOut}>Sign out</Button></Field>
  </Panel>
}

/** An agent sent this person to sign in. Signed in, they pick its authorization back up. */
function Continue({ account, request }: { account: Account; request: URLSearchParams }) {
  const proceed = useMutation({ mutationFn: () => continueAuthorization(request) })
  return <Panel title="Connect an agent" detail={`An agent is asking to use Centinel as ${account.name}. Continue to see what it wants.`}>
    <Field>
      <Button disabled={proceed.isPending} onClick={() => proceed.mutate()}>Continue</Button>
      <FieldError>{proceed.error?.message}</FieldError>
    </Field>
    <Field><Button variant="outline" onClick={signOut}>Sign in as someone else</Button></Field>
  </Panel>
}

function Panel({ title, detail, children }: { title: string; detail: string; children: ReactNode }) {
  return <FieldGroup>
    <EntryHeading title={title}><p className="text-balance text-sm text-muted-foreground">{detail}</p></EntryHeading>
    {children}
  </FieldGroup>
}
