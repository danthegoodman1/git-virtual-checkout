# Rebuild raw git commit objects from GitHub GraphQL data and check the SHA-1.
import json, subprocess, sys, hashlib
owner, name = sys.argv[1], sys.argv[2]
shas = sys.argv[3:]
q = """query($o:String!,$n:String!,$oid:GitObjectID!){repository(owner:$o,name:$n){object(oid:$oid){... on Commit{
 oid tree{oid} parents(first:100){nodes{oid}} message
 author{name email date} committer{name email date}
 signature{signature payload}}}}}"""
def tz(date):  # GitTimestamp keeps the original offset: 2024-01-02T03:04:05-08:00
    from datetime import datetime
    d = datetime.fromisoformat(date.replace('Z', '+00:00'))
    off = d.utcoffset().total_seconds() // 60
    sign = '-' if off < 0 else '+'; off = abs(int(off))
    return f"{int(d.timestamp())} {sign}{off//60:02d}{off%60:02d}"
for sha in shas:
    r = json.loads(subprocess.check_output(['gh','api','graphql','-f',f'query={q}','-f',f'o={owner}','-f',f'n={name}','-f',f'oid={sha}']))
    c = r['data']['repository']['object']
    hdr = f"tree {c['tree']['oid']}\n" + ''.join(f"parent {p['oid']}\n" for p in c['parents']['nodes'])
    hdr += f"author {c['author']['name']} <{c['author']['email']}> {tz(c['author']['date'])}\n"
    hdr += f"committer {c['committer']['name']} <{c['committer']['email']}> {tz(c['committer']['date'])}\n"
    body = hdr + "\n" + c['message']
    cands = {'msg': body, 'msg+nl': body + "\n"}
    sig = c.get('signature')
    if sig and sig.get('payload') and sig.get('signature'):
        # The payload is the object minus its gpgsig header. Git stores that header
        # after the other headers, with each continuation line indented one space.
        head, _, rest = sig['payload'].partition('\n\n')
        # GraphQL trims a trailing blank line from the armor, so try with and without it.
        armor = sig['signature'][:-1] if sig['signature'].endswith('\n') else sig['signature']
        for k, a in (('signed', armor), ('signed+blank', armor + '\n')):
            cands[k] = head + '\ngpgsig ' + '\n '.join(a.split('\n')) + '\n\n' + rest
    ok = None
    for k, raw in cands.items():
        b = raw.encode()
        h = hashlib.sha1(b"commit %d\0" % len(b) + b).hexdigest()
        if h == sha: ok = k; break
    print(sha[:12], 'parents', len(c['parents']['nodes']), 'signed' if sig else 'unsigned', '->', 'MATCH via '+ok if ok else 'MISMATCH')
