import sys, zlib, os
d = open(sys.argv[1],'rb').read(); out = sys.argv[2]; os.makedirs(out, exist_ok=True)
p = d.find(b'\x78\xda'); n = 0; gaps = []
while 0 <= p < len(d):
    try:
        o = zlib.decompressobj(); x = o.decompress(d[p:]) + o.flush()
    except zlib.error:
        q = d.find(b'\x78', p+1); gaps.append(p); p = q; continue
    q = len(d) - len(o.unused_data)
    ext = 'xml' if x.lstrip()[:1] == b'<' else ('png' if x[:4]==b'\x89PNG' else ('ttf' if x[:4] in (b'\0\1\0\0',b'OTTO',b'true') else 'bin'))
    open(f'{out}/{n:05d}.{ext}','wb').write(x); n += 1
    if q < len(d) and d[q:q+1] != b'\x78': gaps.append((q, d[q:q+40]))
    p = q if d[q:q+1]==b'\x78' else d.find(b'\x78\xda', q)
print('streams', n, 'gaps', len(gaps)); print(gaps[:5])
