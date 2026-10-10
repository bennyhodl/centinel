import { describe, expect, it } from 'vitest'
import { parseCsv } from './csv'

describe('parseCsv', () => {
  it('keeps quoted commas, doubled quotes and line breaks inside one cell', () => {
    const text = 'vendor,memo,amount\r\n"Kimmins, Corp","said ""paid""\nin full",1204880.00\n'
    expect(parseCsv(text)).toEqual([
      ['vendor', 'memo', 'amount'],
      ['Kimmins, Corp', 'said "paid"\nin full', '1204880.00'],
    ])
  })

  it('keeps a last row that has no trailing newline, and empty cells', () => {
    expect(parseCsv('a,b\n1,')).toEqual([['a', 'b'], ['1', '']])
  })
})
