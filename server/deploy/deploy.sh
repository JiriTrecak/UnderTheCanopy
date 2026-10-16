#!/usr/bin/env bash
# Deploys server/ to the match host and switches systemd to it
# (docs/server.md). `deploy.sh rollback` switches back to the previous TypeScript server
# from the old repository, which stays installed in /home/ubuntu/settlers-match.
# Refuses while a match is being played unless FORCE=1.
set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
host=18.134.138.1
key=${MATCHHOST_KEY:-$HOME/.config/under-the-canopy/matchhost.pem}
remote=ubuntu@$host
dir=/home/ubuntu/canopy-matchhost
ssh=(ssh -i "$key" -o StrictHostKeyChecking=accept-new)
rsync_ssh="ssh -i '$key' -o StrictHostKeyChecking=accept-new"

[ -r "$key" ] || { echo "SSH key not found: $key (set MATCHHOST_KEY)" >&2; exit 1; }

health() {
  for _ in {1..20}; do
    if "${ssh[@]}" "$remote" curl -sf http://127.0.0.1:8787/api/health; then echo; return 0; fi
    sleep 1
  done
  echo "the server did not answer on the host" >&2; return 1
}

if [ "${1:-}" = rollback ]; then
  "${ssh[@]}" "$remote" "sudo systemctl disable --now canopy-matchhost && sudo systemctl enable --now settlers-matchhost"
  health
  echo "Rolled back to the old server (settlers-matchhost)."
  exit 0
fi

# Never deploy a loopback-only service without its public TLS front end.
"${ssh[@]}" "$remote" "sudo test -s /etc/letsencrypt/live/canopy-matchhost/fullchain.pem && sudo nginx -t" || {
  echo "Configure HTTPS first: docs/server.md" >&2; exit 1
}

playing=$(curl -fsS --max-time 10 "https://$host/api/rooms" | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).filter(r=>r.state==="playing").length))')
if [ "$playing" != 0 ] && [ "${FORCE:-}" != 1 ]; then
  echo "$playing match(es) in progress; deploying would end them. Retry later or set FORCE=1." >&2
  exit 1
fi

"${ssh[@]}" "$remote" "mkdir -p '$dir/server'"
rsync -az --delete --exclude node_modules -e "$rsync_ssh" \
  "$repo/server/src" "$repo/server/package.json" "$repo/server/package-lock.json" "$repo/server/tsconfig.json" "$repo/server/deploy" \
  "$remote:$dir/server/"

"${ssh[@]}" "$remote" bash -s <<'REMOTE'
set -euo pipefail
cd /home/ubuntu/canopy-matchhost/server
npm ci --omit=dev --no-audit --no-fund
sudo cp deploy/canopy-matchhost.service /etc/systemd/system/canopy-matchhost.service
sudo cp deploy/nginx.conf /etc/nginx/sites-available/canopy-matchhost
sudo ln -sfn /etc/nginx/sites-available/canopy-matchhost /etc/nginx/sites-enabled/canopy-matchhost
sudo nginx -t
sudo systemctl reload nginx
sudo systemctl daemon-reload
# Both bind 127.0.0.1:8787: the old service stops, stays installed for rollback.
sudo systemctl disable --now settlers-matchhost 2>/dev/null || true
sudo systemctl enable canopy-matchhost
sudo systemctl restart canopy-matchhost
REMOTE

health
"${ssh[@]}" "$remote" "sudo systemctl is-active canopy-matchhost"
echo "Public: https://$host/api/health"
curl -fsS --max-time 10 "https://$host/api/health"; echo
