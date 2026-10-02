#!/bin/sh
# LOOM_REFERENCE_TRANSACTION_HOOK
#
# Installed into .git/hooks by `loom init`, which overwrites it on update. Each move
# of a target loom guards (a `ref` line of .loom/work/target-guard.refs) is appended
# to .loom/work/target-guard.ledger: `attest <from> <to> <ref>` when git prepares it,
# `abort <from> <to> <ref>` when git aborts it. A loom session's sandbox cannot write
# the ledger: its move is refused unless it only fast-forwards the target through
# paths under the `allow` prefix. Silent on success: git shows hook output.

skip() { cat >/dev/null; exit 0; }

phase=$1
case "$phase" in prepared | aborted) ;; *) skip ;; esac
# Ignore replace refs and grafts, as loom's own git does.
export GIT_NO_REPLACE_OBJECTS=1 GIT_GRAFT_FILE=/dev/null/loom-no-grafts

common=$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null) || skip
work="$(dirname "$common")/.loom/work"
refs="$work/target-guard.refs"
ledger="$work/target-guard.ledger"
[ -f "$refs" ] || skip

nl='
'
guarded=''
allow=''
while IFS= read -r line; do
	case "$line" in
	"ref "*) guarded="$guarded$nl${line#"ref "}" ;;
	"allow "*) allow=${line#"allow "} ;;
	esac
done <"$refs"

# Whether $cur..$new fast-forwards and changes only paths under $allow. Git
# quotes an unusual path, which then starts with `"` and fails the check.
knowledge_only() {
	[ -n "$cur" ] && [ -n "$allow" ] && [ "$new" != "$zero" ] || return 1
	git merge-base --is-ancestor "$cur" "$new" 2>/dev/null || return 1
	paths=$(git -c core.quotePath=true diff --name-only --no-renames "$cur" "$new" \
		2>/dev/null) || return 1
	[ -n "$paths" ] || return 0
	while IFS= read -r path; do
		case "$path" in "$allow"*) ;; *) return 1 ;; esac
	done <<EOF
$paths
EOF
}

refuse() {
	printf 'loom: refusing to move %s from loom session %s: %s %s %s\n' \
		"$ref" "$LOOM_SESSION_ID" \
		'this session cannot record the move for the operator.' \
		"Loom merges your stage branch into the target after 'loom stage complete';" \
		'a move made outside loom holds every merge until the operator reviews it.' >&2
	refused=1
}

# Append a `$1 <from> <to> <ref>` line; fails when the ledger is not writable.
append() {
	(printf '%s %s %s %s\n' "$1" "${cur:-$zero}" "$new" "$ref" >>"$ledger") 2>/dev/null
}

refused=0
# Git passes the all-zero id as the old value (`_`) when the caller gave none
# (`update-ref <ref> <new>`, `branch -f`), so a line carries the ref's current
# value instead; after an abort the ref still holds that value.
while read -r _ new ref; do
	case "$new" in ref:*) continue ;; esac
	case "$guarded$nl" in *"$nl$ref$nl"*) ;; *) continue ;; esac
	cur=$(git rev-parse -q --verify "$ref^{commit}" 2>/dev/null)
	zero=$(printf '%s\n' "$new" | sed 's/[0-9a-f]/0/g')
	if [ "$phase" = aborted ]; then
		append abort
	elif ! append attest && [ -n "$LOOM_SESSION_ID" ] && ! knowledge_only; then
		refuse
	fi
done
exit "$refused"
