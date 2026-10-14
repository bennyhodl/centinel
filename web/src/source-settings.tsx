import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { api, type SourceDefinition, type SourceSettingsOptions } from './api'
import { queries } from './queries'
import { ErrorBox, Spinner } from './ui'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Field, FieldDescription, FieldGroup, FieldLabel, FieldSet } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'

/** A snapshot of the definition initializes the form once, independent of page polling. */
export function SourceSettings({ definition, options, onClose }: { definition: SourceDefinition; options: SourceSettingsOptions; onClose: () => void }) {
  const client = useQueryClient()
  const [rps, setRps] = useState(definition.rps?.toString() ?? '')
  const [matches, setMatches] = useState(definition.matches?.join('\n') ?? '')
  const [strategy, setStrategy] = useState(definition.strategy ?? '')
  const [lang, setLang] = useState(definition.lang ?? '')
  const [audio, setAudio] = useState(definition.audio_if_no_captions === undefined ? 'default' : String(definition.audio_if_no_captions))
  const save = useMutation({
    mutationFn: () => api.updateSource(definition.id, {
      rps: rps === '' ? null : Number(rps),
      matches: matches.split('\n').filter(value => value.trim().length > 0),
      strategy: strategy || null,
      lang: lang || null,
      audio_if_no_captions: audio === 'default' ? null : audio === 'true',
    }),
    onSuccess: async () => {
      await Promise.all([
        client.invalidateQueries({ queryKey: queries.source(definition.id).queryKey }),
        client.invalidateQueries({ queryKey: ['sources'] }),
      ])
      onClose()
    },
  })
  const site = Boolean(definition.site)
  return <Dialog open onOpenChange={next => !next && !save.isPending && onClose()}>
    <DialogContent className="max-h-[90dvh] overflow-y-auto">
      <form onSubmit={event => { event.preventDefault(); save.mutate() }} className="grid gap-5">
        <DialogHeader>
          <DialogTitle>Source settings</DialogTitle>
          <DialogDescription>{definition.id}. Changes apply to the next run. A run already started keeps its settings.</DialogDescription>
        </DialogHeader>
        <FieldSet disabled={save.isPending}><FieldGroup>
          {site && <>
            <Field>
              <FieldLabel htmlFor="source-rps">Requests per second</FieldLabel>
              <Input id="source-rps" type="number" step="any" min="0" value={rps} onChange={event => setRps(event.target.value)} placeholder={`Default: ${options.defaults.rps}`} />
              <FieldDescription>Leave empty to inherit {options.defaults.rps} requests per second. Overrides must be greater than zero.</FieldDescription>
            </Field>
            <Field>
              <FieldLabel htmlFor="source-strategy">Discovery strategy</FieldLabel>
              <Select value={strategy || 'automatic'} onValueChange={value => setStrategy(value === 'automatic' ? '' : value)}>
                <SelectTrigger id="source-strategy" className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="automatic">Recognise on every run</SelectItem>
                  {options.strategies.map(name => <SelectItem key={name} value={name}>{name}</SelectItem>)}
                  {strategy && !options.strategies.includes(strategy) && <SelectItem value={strategy}>{strategy} (unavailable)</SelectItem>}
                </SelectContent>
              </Select>
              <FieldDescription>Pin a strategy after investigating the address, or let Centinel choose each time.</FieldDescription>
            </Field>
          </>}
          <Field>
            <FieldLabel htmlFor="source-matches">URL substring filters</FieldLabel>
            <Textarea id="source-matches" value={matches} onChange={event => setMatches(event.target.value)} rows={3} placeholder={'/agenda/\nbudget'} className="font-mono text-[13px]" />
            <FieldDescription>One substring per line. Collect a URL when it contains any of them. Leave empty to collect every URL.</FieldDescription>
          </Field>
          {!site && <>
            <Field>
              <FieldLabel htmlFor="source-lang">Caption and transcription language</FieldLabel>
              <Input id="source-lang" value={lang} onChange={event => setLang(event.target.value)} placeholder={`Default: ${options.defaults.lang}`} />
              <FieldDescription>A language code such as en or es. Leave empty to inherit {options.defaults.lang}.</FieldDescription>
            </Field>
            <Field>
              <FieldLabel htmlFor="source-audio">Audio fallback</FieldLabel>
              <Select value={audio} onValueChange={setAudio}>
                <SelectTrigger id="source-audio" className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="default">Default: fetch when captions are missing</SelectItem>
                  <SelectItem value="true">Fetch when captions are missing</SelectItem>
                  <SelectItem value="false">Never fetch audio</SelectItem>
                </SelectContent>
              </Select>
              <FieldDescription>Audio from videos without captions becomes searchable through transcription.</FieldDescription>
            </Field>
          </>}
        </FieldGroup></FieldSet>
        {save.error && <ErrorBox error={save.error} />}
        <DialogFooter>
          <Button type="button" variant="outline" disabled={save.isPending} onClick={onClose}>Cancel</Button>
          <Button type="submit" disabled={save.isPending || (rps !== '' && !(Number(rps) > 0))}>{save.isPending && <Spinner />}Save changes</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
}

