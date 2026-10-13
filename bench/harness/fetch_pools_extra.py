"""Maven (Spring Boot BOM), vcpkg and C pools. python3 -I fetch_pools_extra.py"""
import os, json, re, subprocess, time, urllib.request, hashlib
POOLS = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "corpus", "pools")
def get(url):
    with urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "feluda-bench"}), timeout=60) as r:
        return r.read().decode()
def save(name, items, meta):
    json.dump({"source": meta, "fetched": time.strftime("%Y-%m-%d"), "items": items}, open(f"{POOLS}/{name}.json", "w"), indent=0)
    print(name, len(items))

# Maven: Spring Boot's managed dependency versions (a real, mutually compatible set)
meta = get("https://repo1.maven.org/maven2/org/springframework/boot/spring-boot-dependencies/maven-metadata.xml")
ver = re.search(r"<release>([^<]+)</release>", meta).group(1)
pom = get(f"https://repo1.maven.org/maven2/org/springframework/boot/spring-boot-dependencies/{ver}/spring-boot-dependencies-{ver}.pom")
props = dict(re.findall(r"<([\w.\-]+)>([^<$]+)</\1>", pom.split("<dependencyManagement>")[0]))
dm = pom.split("<dependencyManagement>")[1].split("</dependencyManagement>")[0]
items = []
for blk in re.findall(r"<dependency>(.*?)</dependency>", dm, re.S):
    g = re.search(r"<groupId>([^<]+)", blk); a = re.search(r"<artifactId>([^<]+)", blk); v = re.search(r"<version>([^<]+)", blk)
    t = re.search(r"<type>([^<]+)", blk); sc = re.search(r"<scope>([^<]+)", blk)
    if not (g and a and v) or (t and t.group(1) == "pom") or (sc and sc.group(1) == "import"):
        continue
    version = re.sub(r"\$\{([^}]+)\}", lambda m: props.get(m.group(1), m.group(0)), v.group(1))
    if "${" in version or "SNAPSHOT" in version:
        continue
    items.append([g.group(1), a.group(1), version])
seen = set(); uniq = []
for g, a, v in items:
    if (g, a) not in seen:
        seen.add((g, a)); uniq.append([g, a, v])
# stable popular-first order: well known groups first, then the rest by hash
pri = ["org.springframework", "com.fasterxml.jackson", "org.apache", "org.slf4j", "ch.qos.logback", "junit", "org.junit", "com.google", "org.hibernate", "io.netty", "org.mockito", "com.squareup", "commons-", "org.projectlombok"]
def rank(it):
    for i, p in enumerate(pri):
        if it[0].startswith(p):
            return (i, hashlib.md5(it[1].encode()).hexdigest())
    return (len(pri), hashlib.md5(it[1].encode()).hexdigest())
uniq.sort(key=rank)
save("maven", uniq, f"spring-boot-dependencies {ver} managed versions")

# vcpkg
out = subprocess.run(["gh", "api", "repos/microsoft/vcpkg/git/trees/master:ports", "--jq", ".tree[].path"], capture_output=True, text=True).stdout.split()
cur = ["fmt", "spdlog", "openssl", "zlib", "curl", "sqlite3", "protobuf", "grpc", "abseil", "gtest", "nlohmann-json", "boost-system", "boost-filesystem", "boost-asio", "boost-thread", "libpng", "libjpeg-turbo", "lz4", "zstd", "bzip2", "xz", "libxml2", "yaml-cpp", "catch2", "benchmark", "eigen3", "opencv", "glfw3", "glew", "sdl2", "freetype", "harfbuzz", "libuv", "libevent", "cpr", "jsoncpp", "rapidjson", "pugixml", "tinyxml2", "re2", "pcre2", "gflags", "glog", "cxxopts", "cli11", "magic-enum", "range-v3", "doctest", "mimalloc"]
cur = [c for c in cur if c in out]
rest = sorted([p for p in out if p not in cur and not p.startswith("vcpkg-")], key=lambda p: hashlib.md5(p.encode()).hexdigest())
save("vcpkg", cur + rest, "microsoft/vcpkg ports, popular first")

clibs = "z m pthread dl ssl crypto curl png jpeg tiff xml2 xslt sqlite3 yaml pcre pcre2 gmp mpfr readline ncurses ncursesw bz2 lzma zstd lz4 event uv ev jansson json-c protobuf-c expat freetype fontconfig X11 Xext Xrender cairo pango glib-2.0 gobject-2.0 gio-2.0 gtk-3 SDL2 GL GLU glut openal vorbis ogg FLAC sndfile avcodec avformat avutil swscale pq mysqlclient hiredis memcached rdkafka zmq nanomsg curl-gnutls gnutls nettle hogweed gcrypt gpg-error sodium argon2 bsd rt util resolv crypt cap acl attr udev dbus-1 systemd pam".split()
save("clibs", clibs, "curated common C libraries")
