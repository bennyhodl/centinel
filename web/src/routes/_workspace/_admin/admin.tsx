import { createFileRoute } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, type Account } from '../../../api'
import { plural } from '../../../format'
import { queries } from '../../../queries'
import { ErrorBox, handle, PageHeader, Portrait, SectionRule } from '../../../ui'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'

export const Route = createFileRoute('/_workspace/_admin/admin')({
  loader: ({ context: { queryClient } }) => queryClient.ensureQueryData(queries.users()),
  pendingComponent: AdminSkeleton,
  component: Admin,
})

const standing: Record<Account['access'], string> = { pending: 'Awaiting approval', approved: 'Approved', revoked: 'Revoked' }

const detail = 'Approve the people who asked to join this archive, or take their access back. Approval applies at once, to the browser and to any agent signed in as them.'

function AdminSkeleton() {
  return <>
    <PageHeader title="Admin" detail={detail} />
    <div aria-busy className="grid max-w-3xl">
      <SectionRule aside={<Skeleton mask="4 accounts" />}>Accounts</SectionRule>
      {Array.from({ length: 4 }, (_, i) => <div key={i} className="flex items-center gap-4 border-b py-4">
        <Skeleton className="size-10 shrink-0 rounded-full" />
        <div className="min-w-0 flex-1">
          <p className="font-semibold"><Skeleton mask="Archive reader" /></p>
          <p className="text-sm"><Skeleton mask="reader@example.org" /></p>
        </div>
        <Skeleton className="h-5 w-20 rounded-md" />
        <Skeleton className="h-8 w-[74px] rounded-md" />
      </div>)}
    </div>
  </>
}

function Admin() {
  const { data: users = [] } = useQuery(queries.users())
  const queryClient = useQueryClient()
  const access = useMutation({
    mutationFn: ({ id, access }: { id: string; access: Account['access'] }) => api.setAccess(id, access),
    onSuccess: () => queryClient.invalidateQueries(queries.users()),
  })
  return <>
    <PageHeader title="Admin" detail={detail} />
    <div className="grid max-w-3xl">
      <SectionRule aside={plural(users.length, 'account')}>Accounts</SectionRule>
      {users.map(user => <div key={user.id} className="flex items-center gap-4 border-b py-4">
        <Portrait account={user} className="size-10 text-lg" />
        <div className="min-w-0 flex-1">
          <p className="font-semibold">{user.name}</p>
          <p className="truncate text-sm text-muted-foreground">{handle(user)}</p>
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
