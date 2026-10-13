import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { Plus } from 'lucide-react'
import { api, type NewSource } from './api'
import { ErrorBox, Segmented, Spinner } from './ui'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'

/** Defines a source. Adding is not collecting: Run on its page is the next step. */
export function AddSource({ initial, onClose }: { initial: NewSource; onClose: () => void }) {
  const client = useQueryClient()
  const navigate = useNavigate()
  const [kind, setKind] = useState<'site' | 'channel'>(initial.channel ? 'channel' : 'site')
  const [id, setId] = useState(initial.id)
  const [address, setAddress] = useState(initial.site ?? initial.channel ?? '')
  const add = useMutation({
    mutationFn: () => api.addSource({ id: id.trim(), [kind]: address.trim(), strategy: initial.strategy }),
    onSuccess: () => {
      client.invalidateQueries({ queryKey: ['sources'] })
      onClose()
      navigate({ to: '/sources/$id', params: { id: id.trim() } })
    },
  })
  return <Dialog open onOpenChange={next => !next && onClose()}>
    <DialogContent>
      <form onSubmit={event => { event.preventDefault(); add.mutate() }} className="grid gap-5">
        <DialogHeader>
          <DialogTitle>Add a source</DialogTitle>
          <DialogDescription>Defines it in the store. Nothing is collected until you run it.</DialogDescription>
        </DialogHeader>
        <FieldGroup>
          <Segmented label="Kind" value={kind} onChange={setKind} options={[['site', 'Website'], ['channel', 'YouTube channel']]} small />
          <Field>
            <FieldLabel htmlFor="source-id">Id</FieldLabel>
            <Input id="source-id" value={id} onChange={event => setId(event.target.value)} placeholder="tampa" required className="font-mono" />
            <FieldDescription>Lowercase letters, digits and dashes. It names the source everywhere and cannot change.</FieldDescription>
          </Field>
          <Field>
            <FieldLabel htmlFor="source-address">{kind === 'site' ? 'Address' : 'Channel'}</FieldLabel>
            <Input id="source-address" value={address} onChange={event => setAddress(event.target.value)} placeholder={kind === 'site' ? 'https://www.tampa.gov/' : 'https://www.youtube.com/@CityofTampa'} required className="font-mono text-[13px]" />
            {initial.strategy && <FieldDescription>Collected by <span className="font-mono">{initial.strategy}</span>, which recognised it.</FieldDescription>}
          </Field>
        </FieldGroup>
        {add.error && <ErrorBox error={add.error} />}
        <DialogFooter>
          <Button type="button" variant="outline" onClick={onClose}>Cancel</Button>
          <Button type="submit" disabled={!id.trim() || !address.trim() || add.isPending}>{add.isPending ? <Spinner /> : <Plus />}Add</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
}

