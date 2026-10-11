// The conformance runner (spec/RUNNERS.md): every vector
// spec/vectors/MANIFEST.json names, walked from the manifest. A vector of a
// kind this runtime claims is run, and must pass — or, marked
// `"expect": "fail"`, must fail its check; a vector the manifest marks
// `rust-only`, or of a kind not yet implemented, is skipped, by name, with
// the reason. It prints, per kind, what it ran and what it skipped, and
// exits non-zero on any failure, on a manifest out of step with the files,
// and on a claimed kind of which nothing ran. A plain executable, so that it
// needs no test framework: `rust/ark/tests/vectors.rs` is the reference
// reading of each check.
//
//   ArkDBTests [VECTORS] [TRACE_CORPUS]
//
// VECTORS defaults to ../spec/vectors; the corpus to ARK_TRACE_CORPUS.
import ArkDB
import Foundation

setvbuf(stdout, nil, _IOLBF, 0)

/// One vector as the manifest lists it.
struct Vector {
    let path: String
    let kind: String
    let form: String
    let side: String
    let expect: String
}

let arguments = CommandLine.arguments
let root = arguments.count > 1 ? arguments[1] : "../spec/vectors"
Checks.root = root

/// The kinds this runtime runs, in the manifest's order of kinds, each with its check.
let claimed: [(String, (Vector, Value) throws -> Void)] = [
    ("codec", { _, v in try Checks.codec(v) }),
    ("order", { _, v in try Checks.order(v) }),
    ("hash", { _, v in try Checks.hash(v) }),
    ("module", { _, v in try Checks.module(v) }),
    ("verify", { _, v in try Checks.verify(v) }),
    ("eval", { x, v in
        switch x.form {
        case "apply": try Checks.evalApply(v)
        case "cases": try Checks.evalCases(v)
        case "raw": try Checks.evalRaw(v)
        default: throw Failed("eval: unknown form \(x.form)")
        }
    }),
    ("views", { _, v in try Checks.views(v) }),
]

/// The kinds no check here runs yet, and why.
let notYet: [String: String] = [
    "protocol": "the protocol client is docs/plan-phones.md round 6",
    "rebase": "the replica is docs/plan-phones.md round 6",
    "trace": "the client is docs/plan-phones.md round 6",
]

var problems = [String]()
var vectors = [Vector]()
do {
    let manifest = try readJson(root + "/MANIFEST.json")
    for it in try manifest.field("vectors").list() {
        vectors.append(Vector(path: try it.field("path").text(), kind: try it.field("kind").text(), form: try it.field("form").text(), side: try it.field("side").text(), expect: try it.field("expect").text()))
    }
} catch {
    print("conformance: cannot read \(root)/MANIFEST.json: \(error)")
    exit(1)
}

// The manifest is the list: a file it names must be there, and a `.json` it
// does not name is a manifest out of date.
let named = Set(vectors.map { $0.path })
if named.count != vectors.count { problems.append("MANIFEST.json lists a path twice") }
for p in named.sorted() where !FileManager.default.fileExists(atPath: root + "/" + p) {
    problems.append("\(p) is listed and is not there")
}
if let walker = FileManager.default.enumerator(atPath: root) {
    var onDisk = [String]()
    while let p = walker.nextObject() as? String {
        if p.hasSuffix(".json") && p != "MANIFEST.json" { onDisk.append(p) }
    }
    for p in onDisk.sorted() where !named.contains(p) { problems.append("\(p) is a vector the manifest does not name") }
}

// A check's first unmet claim, or nil, and whether what went wrong was a bug.
// What the runtime itself rejects — bytes that are not canonical, a module of
// the wrong shape — is a claim unmet too; anything else thrown is a bug in
// this runtime and is reported as one, so that a falsify vector cannot pass
// by crashing.
func run(_ v: Vector, _ check: (Vector, Value) throws -> Void) -> (String?, Bool) {
    do {
        try check(v, try readJson(root + "/" + v.path))
        return (nil, false)
    } catch let e as Failed {
        return (e.message, false)
    } catch let e as DecodeError {
        return ("decode: \(e)", false)
    } catch let e as ModuleError {
        return ("module: \(e)", false)
    } catch let e as VerifyFailed {
        return ("verify: \(e)", false)
    } catch let e as FactError {
        return ("frame: \(e)", false)
    } catch let e as JsonError {
        return ("json: \(e)", true)
    } catch {
        return ("\(type(of: error)): \(error)", true)
    }
}

var ran = [String: [String]]()
var skipped = [String: [String]]()
for v in vectors {
    let check = claimed.first { $0.0 == v.kind }?.1
    if v.side != "client" {
        skipped[v.kind, default: []].append("\(v.path) (\(v.side): Rust's simulator and its scheduler)")
    } else if let check = check {
        let (failure, bug) = run(v, check)
        var line: String
        if bug {
            line = "\(v.path): BUG: \(failure!)"
            problems.append(line)
        } else if v.expect == "pass" && failure == nil {
            line = "\(v.path): pass"
        } else if v.expect == "fail" && failure != nil {
            line = "\(v.path): fails as it must (\(failure!.prefix(160)))"
        } else if v.expect == "pass" {
            line = "\(v.path): FAILED: \(failure!)"
            problems.append(line)
        } else {
            line = "\(v.path): FAILED: a falsify vector passed"
            problems.append(line)
        }
        ran[v.kind, default: []].append(line)
    } else {
        skipped[v.kind, default: []].append("\(v.path) (kind not implemented: \(notYet[v.kind] ?? "unknown kind"))")
    }
}

var kinds = [String]()
for v in vectors where !kinds.contains(v.kind) { kinds.append(v.kind) }
for k in kinds {
    let r = ran[k] ?? []
    let s = skipped[k] ?? []
    print("\(k): ran \(r.count), skipped \(s.count)")
    for line in r { print("  ran     \(line)") }
    for line in s { print("  skipped \(line)") }
}
for (k, _) in claimed where (ran[k] ?? []).isEmpty { problems.append("\(k) is claimed and ran no vector") }

// Past what the vectors reach: the store's index hints held to the scans they stand for.
do {
    let n = try StoreHints.run()
    print("store hints: \(n) reads through the indexes, each the scan filtered and sorted")
} catch {
    problems.append("store hints: \(error)")
}

// R9's kept numbers three levels deep, past what the views vectors reach.
do {
    let (n, patches) = try KeptNumbers.run(root)
    print("kept numbers: \(n) batches over three kept levels, \(patches) patches; every one a fresh hydrate, a fold's answer and a splice")
} catch {
    problems.append("kept numbers: \(error)")
}

let total = ran.values.reduce(0) { $0 + $1.count }
let skippedTotal = skipped.values.reduce(0) { $0 + $1.count }
print("conformance: \(vectors.count) vectors, \(total) run, \(skippedTotal) skipped; claimed: \(claimed.map { $0.0 }.joined(separator: " "))")
if !problems.isEmpty {
    print("conformance: \(problems.count) problem(s)")
    for p in problems { print("  \(p)") }
    exit(1)
}
print("conformance: every claimed vector passed, and every falsify vector failed")
