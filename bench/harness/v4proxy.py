"""Tiny CONNECT proxy on 127.0.0.1:18888 that dials IPv4 only and skips dead addresses.
This machine's IPv6 is blackholed, and some round robin addresses (for example
185.199.109.133 for raw.githubusercontent.com) never answer. reqwest has no connect timeout, so a
client that draws a dead address sits out its whole 30 s request timeout. Benchmark runs go through
this proxy (1.5 s per address, dead ones remembered) so that network fault stays out of the numbers.
Run: python3 -I v4proxy.py"""
import asyncio, socket, sys

PORT = 18888

BAD = {}

import time

async def addrs(host, port):
    loop = asyncio.get_running_loop()
    infos = await loop.getaddrinfo(host, port, family=socket.AF_INET, type=socket.SOCK_STREAM)
    out = []
    for i in infos:
        if i[4][0] not in out:
            out.append(i[4][0])
    now = time.time()
    good = [a for a in out if BAD.get((host, a), 0) < now]
    bad = [a for a in out if a not in good]
    return good + bad

async def dial(host, port):
    loop = asyncio.get_running_loop()
    infos = await loop.getaddrinfo(host, port, family=socket.AF_INET, type=socket.SOCK_STREAM)
    addrs = [i[4][0] for i in infos]
    import time
    good = [a for a in addrs if BAD.get(a, 0) < time.time()] or addrs
    last = None
    for a in good:
        try:
            return await asyncio.wait_for(asyncio.open_connection(a, port), 1.5)
        except Exception as e:
            BAD[a] = time.time() + 600
            last = e
    raise last or OSError("no address")

async def pipe(r, w):
    try:
        while True:
            d = await r.read(65536)
            if not d:
                break
            w.write(d)
            await w.drain()
    except Exception:
        pass
    finally:
        try:
            w.close()
        except Exception:
            pass

async def handle(cr, cw):
    try:
        head = await cr.readuntil(b"\r\n\r\n")
        line = head.split(b"\r\n")[0].decode()
        method, target, _ = line.split(" ", 2)
        if method != "CONNECT":
            # plain http: forward absolute-URI request
            from urllib.parse import urlsplit
            u = urlsplit(target)
            host, port = u.hostname, u.port or 80
            path = (u.path or "/") + ("?" + u.query if u.query else "")
            rest = head.split(b"\r\n", 1)[1]
            rr, rw = await dial(host, port)
            rw.write(f"{method} {path} HTTP/1.1\r\n".encode() + rest)
            await rw.drain()
            await asyncio.gather(pipe(cr, rw), pipe(rr, cw))
            return
        host, port = target.rsplit(":", 1)
        port = int(port)
        cw.write(b"HTTP/1.1 200 Connection established\r\n\r\n")
        await cw.drain()
        # Probe each IPv4 address with the client's first flight (the TLS ClientHello): an address
        # that accepts the connection but never answers (seen on search.maven.org's CloudFront
        # edges) is dropped after 2 s and the next one gets the same bytes.
        first = await asyncio.wait_for(cr.read(65536), 30)
        if not first:
            return
        rr = rw = None
        data = b""
        for a in await addrs(host, port):
            try:
                r2, w2 = await asyncio.wait_for(asyncio.open_connection(a, port), 1.5)
                w2.write(first)
                await w2.drain()
                data = await asyncio.wait_for(r2.read(65536), 2.0)
                if data:
                    rr, rw = r2, w2
                    break
                w2.close()
            except Exception:
                pass
            BAD[(host, a)] = time.time() + 600
        if rr is None:
            raise OSError("no address answered")
        cw.write(data)
        await cw.drain()
        await asyncio.gather(pipe(cr, rw), pipe(rr, cw))
        return
    except Exception:
        try:
            cw.write(b"HTTP/1.1 502 Bad Gateway\r\n\r\n")
            await cw.drain()
        except Exception:
            pass
    finally:
        try:
            cw.close()
        except Exception:
            pass

async def main():
    srv = await asyncio.start_server(handle, "127.0.0.1", PORT)
    async with srv:
        await srv.serve_forever()

asyncio.run(main())
