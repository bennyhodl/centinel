### M-{{ id }}: {{ query_label }}

- **Run:** {{ run_date }}
- **Asked by:** {{ asked_by }}
- **Used in:** {{ used_in or "(not yet cited)" }}
- **Rows returned:** {{ row_count }}
- **Result hash:** `{{ result_hash }}`

```sql
{{ sql }}
```

{% if notes %}
**Notes / caveats:** {{ notes }}
{% endif %}

---

*This entry is immutable. If a query needs correction, file a new methodology row that supersedes this one and reference both in the published correction.*
