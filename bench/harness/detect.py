"""Which languages feluda sees at a directory's top level (mirrors Language::from_file_name)."""
import os
def detect(d):
    names = set(os.listdir(d))
    L = []
    def has(*n): return any(x in names for x in n)
    if has("Cargo.toml"): L.append("rust")
    if has("package.json", "pnpm-lock.yaml"): L.append("node")
    if has("go.mod", "go.work"): L.append("go")
    if has("requirements.txt", "Pipfile.lock", "pip_freeze.txt", "pyproject.toml"): L.append("python")
    if has("configure.ac", "configure.in", "Makefile"): L.append("c")
    if has("vcpkg.json", "conanfile.txt", "conanfile.py", "CMakeLists.txt", "MODULE.bazel"): L.append("cpp")
    if has("DESCRIPTION", "renv.lock"): L.append("r")
    if any(n.endswith((".csproj", ".fsproj", ".vbproj", ".slnx")) for n in names): L.append("dotnet")
    if has("pom.xml", "build.gradle", "build.gradle.kts"): L.append("java")
    if has("Gemfile.lock", "Gemfile"): L.append("ruby")
    return L
if __name__ == "__main__":
    import sys
    for d in sys.argv[1:]:
        print(os.path.basename(d), detect(d))
