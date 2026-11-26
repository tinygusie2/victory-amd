#!/bin/bash
# Prints NEW <repo> <old> <new> for every watched repo whose head moved; updates the state file.
st=$(dirname "$0")/update-heads.txt; touch $st
for r in "Marvisak/VIctory main" "fxceless/victory-fork main" "NitricAcidDev/VIctory-Omega-Fork primary" "avatarmanz/VIctory-7900XTX main" "DoubleGhosty/cool_fox main"; do
  set -- $r
  h=$(GIT_TERMINAL_PROMPT=0 git ls-remote https://github.com/$1.git refs/heads/$2 2>/dev/null | cut -c1-7)
  [ -z "$h" ] && { echo "GONE $1"; continue; }
  old=$(grep "^$1 " $st | cut -d' ' -f2)
  if [ "$old" != "$h" ]; then echo "NEW $1 ${old:-?} $h"; grep -v "^$1 " $st > $st.t; echo "$1 $h" >> $st.t; mv $st.t $st; fi
done
# PortWatch news (https://portwatch-vi.xyz/#news): print new items once
seen=$(dirname "$0")/portwatch-seen.txt; touch $seen
curl -sL --max-time 30 https://portwatch-vi.xyz/data/tracker.json | python -I -c "
import json,sys
seen=set(open(sys.argv[1]).read().split())
try: n=json.load(sys.stdin)['news']
except Exception: print('PORTWATCH unreadable'); sys.exit()
new=[x for x in n if x.get('id') not in seen]
for x in new: print('NEWS', x.get('date'), '|', x.get('title'), '|', (x.get('dek') or '')[:200])
open(sys.argv[1],'a').write(''.join(x['id']+chr(10) for x in new if x.get('id')))
" $seen
echo DONE
