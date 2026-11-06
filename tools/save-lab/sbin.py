"""Strict, lossless SBIN v2 reader and experimental graph editor (stdlib only).

Edits retain original DATA and append relocated objects. Existing string and
object IDs stay valid; imported IDs are always resolved by name/local graph ID.
"""
from __future__ import annotations

from dataclasses import dataclass
from bisect import bisect_right
import copy
import hashlib
import math
import struct


class SaveError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise SaveError(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def fnv1(data):
    value = 0x811C9DC5
    for byte in data:
        value = ((value * 0x01000193) & 0xFFFFFFFF) ^ byte
    return value


WIDTHS = {5: 4, 6: 4, 9: 2, 10: 4, 13: 2, 15: 4}
MAX_SIZE = 16 * 1024 * 1024
MAX_NODES = 65535


def escape(name):
    return name.replace("~", "~0").replace("/", "~1")


def path_parts(path):
    if not path:
        return []
    path = path[1:] if path.startswith("/") else path
    return [part.replace("~1", "/").replace("~0", "~") for part in path.split("/")]


@dataclass
class Section:
    tag: bytes
    payload: bytes
    padding: bytes
    header_offset: int


@dataclass
class Value:
    type: int
    offset: int  # relative to DATA
    width: int
    value: object


@dataclass
class Node:
    index: int
    offset: int
    end: int
    kind: str
    fields: dict | None = None
    element_type: int | None = None
    items: list | None = None


class Document:
    def __init__(self, raw):
        require(8 <= len(raw) <= MAX_SIZE, "unsupported file size")
        require(raw[:8] == b"SBIN\x02\0\0\0", "expected SBIN v2")
        self.original = bytes(raw)
        self.sections = []
        self.by_tag = {}
        cursor = 8
        while cursor < len(raw):
            require(cursor + 12 <= len(raw), "truncated section header")
            tag, size, checksum = struct.unpack_from("<4sII", raw, cursor)
            end = cursor + 12 + size
            aligned = (end + 3) & ~3
            require(aligned <= len(raw), "section exceeds file")
            require(tag not in self.by_tag, "duplicate section")
            payload = raw[cursor + 12:end]
            require(fnv1(payload) == checksum, f"bad {tag!r} checksum")
            section = Section(tag, payload, raw[end:aligned], cursor)
            self.sections.append(section)
            self.by_tag[tag] = section
            cursor = aligned
        require(all(tag in self.by_tag for tag in (b"DATA", b"OHDR", b"STRS")), "missing core section")
        self.data = self.by_tag[b"DATA"].payload
        string_bytes = self.by_tag[b"STRS"].payload
        require(len(string_bytes) >= 4, "missing string count")
        count = struct.unpack_from("<I", string_bytes)[0]
        require(count < 65536, "too many strings")
        self.strings = []
        cursor = 4
        for _ in range(count):
            require(cursor + 4 <= len(string_bytes), "truncated string length")
            size = struct.unpack_from("<I", string_bytes, cursor)[0]
            cursor += 4
            require(cursor + size <= len(string_bytes), "truncated string")
            try:
                self.strings.append(string_bytes[cursor:cursor + size].decode("utf-8"))
            except UnicodeDecodeError as error:
                raise SaveError("invalid UTF-8 string") from error
            cursor += size
        require(cursor == len(string_bytes), "extra string-table bytes")
        table = self.by_tag[b"OHDR"].payload
        require(table and len(table) % 4 == 0, "invalid object table")
        self.entries = list(struct.unpack("<" + "I" * (len(table) // 4), table))
        require(len(self.entries) <= MAX_NODES, "too many objects")
        self.ends = sorted({entry >> 3 for entry in self.entries} | {len(self.data)})
        self.nodes = {}
        for index, entry in enumerate(self.entries):
            require(entry & 7 in (1, 2) and (entry >> 3) + 4 <= len(self.data), f"bad OHDR[{index}]")
        require(self.entries[0] & 7 == 1, "root must be OHDR[0] object")
        # Validate every entry, including unreachable nodes retained by edits.
        for index in range(len(self.entries)):
            self.node(index)
        self.paths = self.walk()  # root traversal checks cycles and depth

    def name(self, index):
        require(1 <= index <= len(self.strings), f"invalid string ID {index}")
        return self.strings[index - 1]

    def u32(self, offset):
        require(0 <= offset <= len(self.data) - 4, "u32 outside DATA")
        return struct.unpack_from("<I", self.data, offset)[0]

    def value(self, kind, offset, end):
        require(kind in WIDTHS, f"unsupported scalar type 0x{kind:x}")
        width = WIDTHS[kind]
        owned = min(width, end - offset)
        require(offset >= 0 and owned >= (1 if kind == 9 else width), "value outside object")
        if kind == 9:
            # A 13-byte boolean object owns only its low byte; preserve CD padding.
            value = self.data[offset]
        elif kind == 13:
            value = self.name(struct.unpack_from("<H", self.data, offset)[0])
        elif kind == 10:
            value = struct.unpack_from("<f", self.data, offset)[0]
        else:
            value = self.u32(offset)
            if kind == 15:
                require(value < len(self.entries), "reference outside OHDR")
        return Value(kind, offset, owned, value)

    def node(self, index):
        require(isinstance(index, int) and 0 <= index < len(self.entries), "invalid object reference")
        if index in self.nodes:
            return self.nodes[index]
        entry = self.entries[index]
        start, flag = entry >> 3, entry & 7
        boundary = self.ends[bisect_right(self.ends, start)]
        if flag == 1:
            header = self.u32(start)
            size, count = header >> 16, header & 65535
            end = start + size
            require(size >= 4 and end <= boundary, f"object {index} overlaps next entry")
            fields = {}
            cursor = start + 4
            for _ in range(count):
                require(cursor + 8 <= end, f"object {index} field header overflow")
                name_id, kind, relative = struct.unpack_from("<HHI", self.data, cursor)
                name = self.name(name_id)
                require(name not in fields, "duplicate object field")
                offset = start + relative
                require(cursor + 8 <= offset <= cursor + 10, "unexpected field layout")
                fields[name] = self.value(kind, offset, end)
                cursor = offset + WIDTHS[kind]
            require(cursor <= end + 1, "object field overflow")
            result = Node(index, start, end, "object", fields=fields)
        else:
            require(start + 8 <= boundary, "truncated list header")
            kind, count = struct.unpack_from("<II", self.data, start)
            require(count <= MAX_NODES, "excessive list count")
            require(kind in WIDTHS or kind == 16, f"unsupported list type 0x{kind:x}")
            width = 8 if kind == 16 else WIDTHS[kind]
            end = start + 8 + count * width
            require(end <= boundary, "list overlaps next entry")
            items = []
            for item in range(count):
                offset = start + 8 + item * width
                if kind == 16:
                    items.append({"type": self.value(13, offset, end),
                                  "count": self.value(5, offset + 4, end)})
                else:
                    items.append(self.value(kind, offset, end))
            result = Node(index, start, end, "list", element_type=kind, items=items)
        self.nodes[index] = result
        return result

    def walk(self):
        result = {}
        visits = 0

        def visit(value, path, active):
            nonlocal visits
            visits += 1
            require(visits <= 200000, "graph expands beyond traversal limit")
            require(len(active) <= 64, "graph too deep")
            result[path] = value
            if value.type != 15:
                return
            index = value.value
            require(index not in active, "cyclic reference graph")
            node = self.node(index)
            active = active | {index}
            if node.kind == "object":
                children = node.fields.items()
            elif node.element_type == 16:
                # Only this known schema gives type 0x10 its inventory meaning.
                require(path == "player/inventory/items", "unknown 0x10 record-list schema")
                children = ((f"{i}/{name}", v) for i, item in enumerate(node.items) for name, v in item.items())
            else:
                children = ((str(i), value) for i, value in enumerate(node.items))
            for name, child in children:
                suffix = name if node.kind == "list" else escape(name)
                visit(child, (path + "/" if path else "") + suffix, active)

        visit(Value(15, -1, 4, 0), "", set())
        return result

    def locate(self, path):
        normalized = "/".join(escape(part) for part in path_parts(path))
        fields = self.paths
        require(normalized in fields, f"path not found: {path}")
        return fields[normalized]

    def summary(self):
        values = self.walk()
        containers = {}
        for path, value in values.items():
            if value.type == 15:
                node = self.node(value.value)
                containers[path] = {"type": 15, "kind": node.kind, "data_offset": node.offset}
                if node.kind == "object":
                    containers[path]["fields"] = sorted(node.fields)
                else:
                    containers[path].update({"element_type": node.element_type, "count": len(node.items)})
        return {"sha256": digest(self.original), "bytes": len(self.original),
                "root_data_offset": self.entries[0] >> 3, "objects": len(self.entries),
                "strings": len(self.strings), "sections": [s.tag.decode("ascii", "replace") for s in self.sections],
                "containers": containers,
                "values": {path: {"type": value.type, "data_offset": value.offset,
                                  "value": value.value} for path, value in values.items() if value.type != 15}}

    def rebuild(self, replacements):
        out = bytearray(self.original[:8])
        for section in self.sections:
            payload = replacements.get(section.tag, section.payload)
            out += struct.pack("<4sII", section.tag, len(payload), fnv1(payload)) + payload
            padding_size = (-len(out)) % 4
            out += section.padding if padding_size == len(section.padding) else b"\0" * padding_size
        require(len(out) <= MAX_SIZE, "edited file exceeds size limit")
        Document(out)
        return bytes(out)

    def edit(self, changes):
        data = bytearray(self.data)
        strings = list(self.strings)
        for path, new_value in changes.items():
            field = self.locate(path)
            require(field.type != 15, "use module import or list replacement for references")
            encoded = encode_scalar(field.type, new_value, strings)
            if field.type == 9:
                encoded = encoded[:1]
            require(len(encoded) <= field.width, "edit exceeds owned field")
            data[field.offset:field.offset + len(encoded)] = encoded
        replacements = {b"DATA": bytes(data)}
        if strings != self.strings:
            replacements[b"STRS"] = pack_strings(strings)
        output = self.rebuild(replacements)
        reread = Document(output)
        for path, expected in changes.items():
            actual = reread.locate(path)
            require(encode_scalar(actual.type, actual.value, strings) == encode_scalar(actual.type, expected, strings),
                    f"edit verification failed: {path}")
        return output

    def export_module(self, path):
        root = self.locate(path)
        nodes = {}

        def export_value(value):
            result = {"type": value.type}
            if value.type != 15:
                result["value"] = value.value
                return result
            key = str(value.value)
            result["ref"] = key
            if key not in nodes:
                node = self.node(value.value)
                encoded = {"kind": node.kind}
                nodes[key] = encoded
                if node.kind == "object":
                    encoded["fields"] = [{"name": name, **export_value(v)} for name, v in node.fields.items()]
                else:
                    encoded["element_type"] = node.element_type
                    if node.element_type == 16:
                        encoded["items"] = [{name: v.value for name, v in item.items()} for item in node.items]
                    else:
                        encoded["items"] = [export_value(v) for v in node.items]
            return result

        return {"format": "deadspace.sbin.module.v1", "source": {"sha256": digest(self.original), "path": path},
                "root": export_value(root), "nodes": nodes}

    def import_module(self, path, module):
        target = self.locate(path)
        require(module.get("format") == "deadspace.sbin.module.v1", "unsupported module format")
        require(module["root"]["type"] == target.type, "module/target type mismatch")
        if target.type != 15:
            return self.edit({path: module["root"]["value"]})
        nodes = module["nodes"]
        require(isinstance(nodes, dict) and 0 < len(nodes) <= MAX_NODES - len(self.entries), "invalid module object count")
        require(module["root"]["ref"] in nodes, "missing module root")
        root_node = nodes[module["root"]["ref"]]
        original_node = self.node(target.value)
        require(root_node["kind"] == original_node.kind, "module/target object kind mismatch")
        if original_node.kind == "list":
            require(root_node["element_type"] == original_node.element_type, "list element type mismatch")
        strings = list(self.strings)
        entries = list(self.entries)
        data = bytearray(self.data)
        remap = {key: len(entries) + i for i, key in enumerate(nodes)}
        active, done = set(), set()

        def validate_graph(key, depth=0):
            require(key in nodes and key not in active and depth <= 64, "missing/cyclic/deep module reference")
            if key in done:
                return
            active.add(key)
            node = nodes[key]
            values = node.get("fields", []) if node["kind"] == "object" else node.get("items", [])
            if node.get("element_type") != 16:
                for value in values:
                    if value.get("type") == 15:
                        validate_graph(value["ref"], depth + 1)
            active.remove(key)
            done.add(key)

        validate_graph(module["root"]["ref"])
        require(done == set(nodes), "module contains unreachable nodes")

        def encoded_value(value):
            kind = value["type"]
            if kind == 15:
                require(value["ref"] in remap, "unknown module reference")
                return struct.pack("<I", remap[value["ref"]])
            return encode_scalar(kind, value["value"], strings)

        for key, node in nodes.items():
            require(node["kind"] in ("object", "list"), "unsupported module node kind")
            if node["kind"] == "object":
                fields = node["fields"]
                require(len(fields) <= 65535, "too many object fields")
                body = bytearray()
                seen = set()
                for field in fields:
                    name = field["name"]
                    require(name not in seen, "duplicate module field")
                    seen.add(name)
                    name_id = string_id(name, strings)
                    encoded = encoded_value(field)
                    relative = 4 + len(body) + 8
                    body += struct.pack("<HHI", name_id, field["type"], relative) + encoded
                require(4 + len(body) <= 65535, "object exceeds u16 size")
                payload = struct.pack("<I", len(fields) | ((4 + len(body)) << 16)) + body
                flag = 1
            else:
                kind, items = node["element_type"], node["items"]
                require(kind in WIDTHS or kind == 16, "unsupported module list type")
                require(len(items) <= MAX_NODES, "too many list items")
                body = bytearray()
                for item in items:
                    if kind == 16:
                        body += encode_scalar(13, item["type"], strings) + b"\0\0" + encode_scalar(5, item["count"], strings)
                    else:
                        require(item["type"] == kind, "list element type mismatch")
                        body += encoded_value(item)
                payload = struct.pack("<II", kind, len(items)) + body
                flag = 2
            data += b"\0" * ((-len(data)) % 4)
            entries.append((len(data) << 3) | flag)
            data += payload
        imported = remap[module["root"]["ref"]]
        if target.offset < 0:
            entries[0] = entries[imported]
        else:
            struct.pack_into("<I", data, target.offset, imported)
        return self.rebuild({b"DATA": bytes(data), b"OHDR": struct.pack("<" + "I" * len(entries), *entries),
                             b"STRS": pack_strings(strings)})

    def replace_list(self, path, items):
        module = self.export_module(path)
        root = module["root"]
        require(root["type"] == 15, "target is not a list reference")
        node = module["nodes"][root["ref"]]
        require(node["kind"] == "list" and node["element_type"] != 15, "use modules for object-reference lists")
        kind = node["element_type"]
        node["items"] = items if kind == 16 else [{"type": kind, "value": item} for item in items]
        return self.import_module(path, module)

    def attach(self, parent_path, name, module):
        parent = self.export_module(parent_path)
        root = parent["root"]
        require(root["type"] == 15, "parent must be object reference")
        node = parent["nodes"][root["ref"]]
        require(node["kind"] == "object", "parent must be object")
        require(all(f["name"] != name for f in node["fields"]), "field already exists; import to replace it")
        require(module.get("format") == parent["format"], "unsupported module format")
        child = copy.deepcopy(module)
        mapping = {key: "attached-" + key for key in child["nodes"]}
        for child_node in child["nodes"].values():
            values = child_node.get("fields", []) if child_node["kind"] == "object" else child_node.get("items", [])
            for value in values:
                if isinstance(value, dict) and value.get("type") == 15:
                    value["ref"] = mapping[value["ref"]]
        if child["root"]["type"] == 15:
            child["root"]["ref"] = mapping[child["root"]["ref"]]
        parent["nodes"].update({mapping[key]: value for key, value in child["nodes"].items()})
        node["fields"].append({"name": name, **child["root"]})
        return self.import_module(parent_path, parent)

    def detach(self, path):
        parts = path_parts(path)
        require(parts, "cannot detach the document root")
        parent_path = "/".join(escape(part) for part in parts[:-1])
        parent = self.export_module(parent_path)
        node = parent["nodes"][parent["root"]["ref"]]
        require(node["kind"] == "object", "detach supports named object fields")
        require(any(field["name"] == parts[-1] for field in node["fields"]), "field not found")
        node["fields"] = [field for field in node["fields"] if field["name"] != parts[-1]]
        # Prune the now-unreachable local graph without changing any source IDs.
        keep = set()

        def mark(value):
            if value["type"] != 15 or value["ref"] in keep:
                return
            keep.add(value["ref"])
            current = parent["nodes"][value["ref"]]
            values = current.get("fields", []) if current["kind"] == "object" else current.get("items", [])
            if current.get("element_type") != 16:
                for child in values:
                    mark(child)

        mark(parent["root"])
        parent["nodes"] = {key: value for key, value in parent["nodes"].items() if key in keep}
        return self.import_module(parent_path, parent)


def string_id(value, strings):
    require(isinstance(value, str) and len(value.encode("utf-8")) <= 1024 * 1024, "invalid string")
    if value not in strings:
        require(len(strings) < 65535, "string table full")
        strings.append(value)
    return strings.index(value) + 1


def encode_scalar(kind, value, strings):
    if kind == 13:
        return struct.pack("<H", string_id(value, strings))
    if kind == 9:
        require(isinstance(value, (bool, int)) and value in (0, 1), "boolean must be 0 or 1")
        return bytes((int(value), 0))
    if kind in (5, 6):
        require(isinstance(value, int) and not isinstance(value, bool) and 0 <= value <= 0xFFFFFFFF, "expected u32 storage value")
        return struct.pack("<I", value)
    if kind == 10:
        require(isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value), "expected finite float32")
        try:
            return struct.pack("<f", value)
        except (OverflowError, struct.error) as error:
            raise SaveError("float32 overflow") from error
    raise SaveError(f"unsupported scalar type: {kind}")


def pack_strings(strings):
    body = bytearray(struct.pack("<I", len(strings)))
    for value in strings:
        encoded = value.encode("utf-8")
        body += struct.pack("<I", len(encoded)) + encoded
    return bytes(body)
