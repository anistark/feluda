#!/bin/bash
# summarise a raw dir: rows, unknown count, per host requests/statuses/errors
d=$1
echo "== $d rows=$(jq length $d/report.json 2>/dev/null) unknown=$(jq '[.[]|select((.license//"")|test("nknown|NOASSERTION|^$|No License";"i"))]|length' $d/report.json 2>/dev/null) ecosystems=$(jq -c '[.[].ecosystem]|group_by(.)|map({(.[0]):length})|add' $d/report.json 2>/dev/null)"
jq -r '.hosts|to_entries[]|"   \(.key) n=\(.value.requests) st=\(.value.statuses|tostring) err=\(.value.transport_errors) total_s=\(.value.total_ms/1000|floor) max_s=\(.value.max_ms/1000|floor)"' $d/requests.json 2>/dev/null
