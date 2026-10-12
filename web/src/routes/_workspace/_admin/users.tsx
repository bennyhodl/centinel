import { createFileRoute } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { nip19 } from 'nostr-tools'
import { api, type Account } from '../../../api'
import { plural } from '../../../format'
import { queries } from '../../../queries'
import { ErrorBox, PageHeader, SectionRule } from '../../../ui'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'

export const Route = createFileRoute('/_workspace/_admin/users')({
  loader: ({ context: { queryClient } }) => queryClient.ensureQueryData(queries.users()),
  component: UserAccess,
})

const standing: Record<Account['access'], string> = { pending: 'Awaiting approval', approved: 'Approved', revoked: 'Revoked' }

function UserAccess() {
  const { data: users = [] } = useQuery(queries.users())
  const queryClient = useQueryClient()
  const access = useMutation({
    mutationFn: ({ id, access }: { id: string; access: Account['access'] }) => api.setAccess(id, access),
    onSuccess: () => queryClient.invalidateQueries(queries.users()),
  })
  return <>
    <PageHeader title="User access" detail="Approve the people who asked to join this archive, or take their access back. Approval applies at once, to the browser and to any agent signed in as them." />
    <div className="grid max-w-3xl">
      <SectionRule aside={plural(users.length, 'account')}>Accounts</SectionRule>
      {users.map(user => <div key={user.id} className="flex items-center gap-4 border-b py-4">
        <Portrait account={user} />
        <div className="min-w-0 flex-1">
          <p className="font-semibold">{user.name}</p>
          <p className="truncate text-sm text-muted-foreground">{user.publicKey ? nip19.npubEncode(user.publicKey) : user.email}</p>
        </div>
        <Badge variant={user.access === 'approved' ? 'secondary' : 'outline'}>{user.role === 'admin' ? 'Administrator' : standing[user.access]}</Badge>
        {user.role !== 'admin' && <Button
          size="sm"
          variant={user.access === 'approved' ? 'outline' : 'default'}
          disabled={access.isPending && access.variables.id === user.id}
          onClick={() => access.mutate({ id: user.id, access: user.access === 'approved' ? 'revoked' : 'approved' })}
        >{user.access === 'approved' ? 'Revoke' : 'Approve'}</Button>}
      </div>)}
      {access.error && <ErrorBox error={access.error} />}
    </div>
  </>
}

function Portrait({ account }: { account: Account }) {
  if (account.image) return <img src={account.image} alt="" className="size-10 shrink-0 rounded-full border object-cover" />
  return <span className="grid size-10 shrink-0 place-items-center rounded-full bg-muted font-serif text-lg">{account.name.trim().charAt(0).toUpperCase() || '?'}</span>
}
