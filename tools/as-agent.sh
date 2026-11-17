#!/bin/sh
# Copyright (C) 2026 dekajoule
# SPDX-License-Identifier: GPL-2.0-or-later
# as-agent.sh - run a command as the project's development agent, the GitHub App dekajoule-oos-agent
# (docs/adr/process/0039): gh and git push then act with the App's installation token, so a pull request the agent
# opens is the agent's, not the maintainer's, and tools/pr-gate.sh can tell the two apart.
#
#   tools/as-agent.sh gh pr create ...        any command; GH_TOKEN and git's credentials are the App's
#   tools/as-agent.sh git push -u origin BR
#   tools/as-agent.sh --token                 print the token (for a step that needs it as a value)
#
# Reads the App from $OOS_AGENT_DIR (default ~/.config/oos-agent): app.json (id, installation_id) and key.pem, the
# App's private key, which never goes into the repository. A token lives an hour; it is kept in that folder for 50
# minutes and made again after. Needs openssl, curl and jq. Touches no scope.
set -eu
dir=${OOS_AGENT_DIR:-$HOME/.config/oos-agent}
[ -f "$dir/app.json" ] && [ -f "$dir/key.pem" ] || {
	echo "as-agent.sh: no App in $dir (app.json and key.pem)" >&2
	exit 2
}

b64url() { openssl base64 -A | tr '+/' '-_' | tr -d '='; }

token() {
	now=$(date +%s)
	made=$(stat -f %m "$dir/token" 2>/dev/null || stat -c %Y "$dir/token" 2>/dev/null || echo 0)
	if [ $((now - made)) -lt 3000 ]; then
		cat "$dir/token"
		return
	fi
	app=$(jq -r .id "$dir/app.json")
	inst=$(jq -r .installation_id "$dir/app.json")
	head=$(printf '{"alg":"RS256","typ":"JWT"}' | b64url)
	body=$(printf '{"iat":%d,"exp":%d,"iss":"%s"}' $((now - 60)) $((now + 540)) "$app" | b64url)
	sig=$(printf '%s.%s' "$head" "$body" | openssl dgst -sha256 -sign "$dir/key.pem" -binary | b64url)
	t=$(curl -fsS -X POST -H "Authorization: Bearer $head.$body.$sig" -H 'Accept: application/vnd.github+json' \
		"https://api.github.com/app/installations/$inst/access_tokens" | jq -r .token)
	[ -n "$t" ] && [ "$t" != null ] || {
		echo "as-agent.sh: GitHub gave no token" >&2
		exit 1
	}
	(umask 077 && printf '%s\n' "$t" >"$dir/token")
	printf '%s\n' "$t"
}

case ${1:-} in
'') echo "usage: $0 COMMAND [ARGS...] | --token" >&2; exit 2 ;;
--token) token; exit 0 ;;
esac
GH_TOKEN=$(token)
export GH_TOKEN
# git: the App's token for github.com, in place of every helper of the user's configuration (the empty value clears
# the list); the token is read from the environment, never written into a config file
export GIT_CONFIG_COUNT=2
export GIT_CONFIG_KEY_0=credential.https://github.com.helper GIT_CONFIG_VALUE_0=
export GIT_CONFIG_KEY_1=credential.https://github.com.helper
export GIT_CONFIG_VALUE_1='!f() { echo username=x-access-token; echo "password=$GH_TOKEN"; }; f'
exec "$@"
