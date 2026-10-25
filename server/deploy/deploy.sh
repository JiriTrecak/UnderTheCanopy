#!/usr/bin/env bash
# Deploys server/ to the match host and switches systemd to it
# (docs/server.md). `LADDER_SEED=<folder>` also uploads a seeded ladder folder
# (`npm run seed-ladder`, then `npm run ladder-pool`) as the host's ladder, replacing it. `deploy.sh rollback` switches back to the previous TypeScript server
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

# The server runs on its own Node (the newest, for its security fixes and node:sqlite),
# installed beside it from nodejs.org and checked against its published SHA-256; the
# system's Node stays for the host's other services. Raise both lines to update it.
node_version=v26.12.1
node_sha=6170b2d474e1c626f43cff3adc118df9fe3b573258a4fc3d850c8dbc6b079bbd
"${ssh[@]}" "$remote" bash -s -- "$node_version" "$node_sha" <<'REMOTE'
set -euo pipefail
version=$1 sha=$2 home=/home/ubuntu/canopy-matchhost/node
if [ "$("$home/bin/node" --version 2>/dev/null)" != "$version" ]; then
  tarball=node-$version-linux-x64.tar.xz
  tmp=$(mktemp -d)
  curl -fsSL "https://nodejs.org/dist/$version/$tarball" -o "$tmp/$tarball"
  echo "$sha  $tmp/$tarball" | sha256sum -c -
  rm -rf "$home" && mkdir -p "$home"
  tar -xJf "$tmp/$tarball" -C "$home" --strip-components=1
  rm -rf "$tmp"
fi
"$home/bin/node" --version
REMOTE

"${ssh[@]}" "$remote" "mkdir -p '$dir/server'"
rsync -az --delete --exclude node_modules -e "$rsync_ssh" \
  "$repo/server/src" "$repo/server/package.json" "$repo/server/package-lock.json" "$repo/server/tsconfig.json" "$repo/server/deploy" \
  "$remote:$dir/server/"

"${ssh[@]}" "$remote" bash -s <<'REMOTE'
set -euo pipefail
cd /home/ubuntu/canopy-matchhost/server
export PATH=/home/ubuntu/canopy-matchhost/node/bin:$PATH
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

if [ -n "${LADDER_SEED:-}" ]; then
  [ -s "$LADDER_SEED/ladder.sqlite" ] || { echo "No ladder.sqlite in $LADDER_SEED" >&2; exit 1; }
  "${ssh[@]}" "$remote" "sudo systemctl stop canopy-matchhost && sudo mkdir -p /var/lib/canopy-matchhost/ladder && sudo chown -R ubuntu: /var/lib/canopy-matchhost/ladder"
  rsync -az --delete -e "$rsync_ssh" "$LADDER_SEED/" "$remote:/var/lib/canopy-matchhost/ladder/"
  "${ssh[@]}" "$remote" "sudo systemctl start canopy-matchhost"
fi

health
"${ssh[@]}" "$remote" "sudo systemctl is-active canopy-matchhost"
echo "Public: https://$host/api/health"
curl -fsS --max-time 10 "https://$host/api/health"; echo
