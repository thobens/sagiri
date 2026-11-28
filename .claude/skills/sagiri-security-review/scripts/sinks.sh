#!/usr/bin/env bash
# Lists the places in the Rust sources where data can leave a Sagiri process or where a
# process can stop abnormally. It only finds candidates. Whether a hit can carry pasted
# text, a config entry or key material is decided by reading the code around it.
#
# usage: sinks.sh [--with-tests] [--root DIR] [--path PART] [CATEGORY...]
#
# Categories (default: all):
#   output   print macros, write macros, stdout and stderr handles
#   panic    unwrap, expect, panic, assert and similar
#   slice    range indexing such as text[a..b]; on a str a bad range panics, and the
#            panic message quotes the string
#   debug    Debug derives and impls, and {:?} in format strings
#   process  child processes, their arguments and environment
#   file     files, directories, permissions
#   env      environment variables and process arguments
#   net      sockets of every kind
#   state    statics, caches, collections and zeroizing: what outlives one call
#   arith    shifts and `as` casts: a shift by a computed amount panics in a debug build
#            when the amount is too large, and a cast truncates silently
#
# Lines of test code are left out unless --with-tests is given; then they are marked
# with T. Test code is recognised by `#[cfg(test)]` and the indentation rustfmt gives the
# item after it. That is a heuristic: the number of hidden lines is printed at the end,
# and --with-tests shows them when a count looks wrong.
#
# --root DIR reviews another checkout, for example the worktree of a ticket branch.
# --path PART limits the run to files whose path contains PART, for example
# `rules/ip.rs` or `session/`. The counts at the end then cover those files only.
set -euo pipefail

with_tests=0
root=""
part=""
categories=()
while [ "$#" -gt 0 ]; do
    case "$1" in
        --with-tests) with_tests=1 ;;
        --root)
            shift
            root="${1:?--root needs a directory}"
            ;;
        --path)
            shift
            part="${1:?--path needs a part of a path}"
            ;;
        -h | --help)
            sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        output | panic | slice | debug | process | file | env | net | state | arith)
            categories+=("$1")
            ;;
        *)
            echo "sinks.sh: unknown argument: $1" >&2
            exit 2
            ;;
    esac
    shift
done
if [ "${#categories[@]}" -eq 0 ]; then
    categories=(output panic slice debug process file env net state arith)
fi
if [ -z "$root" ]; then
    root="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
fi
cd "$root"

files="$(find crates -type f -name '*.rs' -path '*/src/*' | LC_ALL=C sort | grep -F -- "$part" || true)"
if [ -z "$files" ]; then
    echo "sinks.sh: no source file matches --path $part" >&2
    exit 2
fi
printf '%s\n' "$files" | xargs awk \
    -v with_tests="$with_tests" -v wanted="${categories[*]}" '
BEGIN {
    n = split(wanted, w, " ")
    for (i = 1; i <= n; i++) want[w[i]] = 1
    re["output"] = "(e?print(ln)?!|write(ln)?!|dbg!|\\.write_all\\(|std(out|err)\\(\\))"
    re["panic"] = "(\\.unwrap\\(\\)|\\.expect\\(|panic!|unreachable!|unimplemented!|todo!|assert(_eq|_ne)?!)"
    re["slice"] = "\\[[^]\"]*\\.\\.[^]\"]*\\]"
    re["debug"] = "(derive\\([^)]*Debug|impl (fmt::)?Debug|\\{[A-Za-z0-9_.]*:#?\\?\\})"
    re["process"] = "(Command::new|process::|current_exe|\\.spawn\\(|\\.args?\\(|\\.envs?\\()"
    re["file"] = "(fs::|File::|OpenOptions|DirBuilder|tempfile|set_permissions|hard_link|remove_file|\\.mode\\()"
    re["env"] = "(env::var|env::args|var_os|current_dir)"
    re["net"] = "(std::net|TcpStream|TcpListener|UdpSocket|UnixStream|UnixListener|UnixDatagram|ToSocketAddrs)"
    re["state"] = "(^ *(pub )?static |LazyLock|OnceLock|thread_local!|HashMap|BTreeMap|HashSet|Box::leak|mem::forget|[Zz]eroiz)"
    re["arith"] = "( (<<|>>)=? | as [ui](8|16|32|64|128|size))"
    order = "output panic slice debug process file env net state arith"
    m = split(order, cats, " ")
}
FNR == 1 { pending = 0; in_test = 0 }
{
    line = $0
    indent = match(line, /[^ ]/) - 1
    is_test = 0
    if (in_test) {
        is_test = 1
        rest = substr(line, indent + 1)
        if (indent == test_indent && rest ~ /^}[;,)]*$/) in_test = 0
    } else if (line ~ /^ *#\[cfg\(test\)\]/) {
        pending = 1
        is_test = 1
    } else if (pending) {
        is_test = 1
        if (line ~ /^ *#\[/) {
            # another attribute of the same item
        } else {
            pending = 0
            # An item on one line (`use x;`, `fn f() { .. }`) ends here.
            if (line !~ /[;}]$/) {
                in_test = 1
                test_indent = indent
            }
        }
    }
    if (line ~ /^ *\/\//) next
    for (i = 1; i <= m; i++) {
        c = cats[i]
        if (!(c in want) || line !~ re[c]) continue
        if (is_test) {
            hidden[c]++
            if (!with_tests) continue
        }
        sub(/^ +/, "", line)
        printf "%-7s %s %s:%d: %s\n", c, (is_test ? "T" : " "), FILENAME, FNR, line
    }
}
END {
    if (with_tests) exit
    for (i = 1; i <= m; i++) {
        c = cats[i]
        if (c in want) printf "# %s: %d line(s) of test code not shown\n", c, hidden[c] + 0
    }
}'
