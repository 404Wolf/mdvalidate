#!/usr/bin/env python3
import sys
from typing import List, Tuple


def parse_parentheses(s: str) -> List[Tuple[str, List[int]]]:
    """Parse a parenthetical expression into nodes with their children indices."""
    nodes: List[Tuple[str, List[int]]] = []
    stack: List[int] = []
    current_name: str = ""
    reading_name: bool = False

    i: int = 0
    while i < len(s):
        if s[i] == "(":
            stack.append(len(nodes))
            reading_name = True
            current_name = ""
        elif s[i] == ")":
            if reading_name and current_name:
                nodes.append((current_name, []))
                reading_name = False
                current_name = ""
            if stack:
                parent_idx: int = stack.pop(-1)
                if len(stack) > 0:
                    parent_parent_idx: int = stack[-1]
                    nodes[parent_parent_idx][1].append(parent_idx)
        elif reading_name and not s[i].isspace():
            current_name += s[i]
        elif reading_name and s[i].isspace() and current_name:
            nodes.append((current_name, []))
            reading_name = False

        i += 1

    return nodes


def build_tree(
    nodes: List[Tuple[str, List[int]]],
    node_idx: int = 0,
    prefix: str = "",
    is_last: bool = True,
) -> List[str]:
    """Build a tree representation from parsed nodes."""
    if not nodes:
        return []

    result: List[str] = []
    node_name, children = nodes[node_idx]

    # Add current node
    connector: str = "└── " if is_last else "├── "
    result.append(f"{prefix}{connector}({node_name})")

    # Prepare prefix for children
    new_prefix: str = prefix + ("    " if is_last else "│   ")

    # Add children
    for i, child_idx in enumerate(children):
        is_last_child: bool = i == len(children) - 1
        result.extend(build_tree(nodes, child_idx, new_prefix, is_last_child))

    return result


def print_tree(s: str) -> None:
    """Parse and print a tree visualization of a parenthetical expression."""
    # Clean input
    s = s.strip()
    if not s or s[0] != "(" or s[-1] != ")":
        print("Invalid input format. Expected parenthetical expression.")
        return

    # Parse into nodes
    nodes = parse_parentheses(s)
    if not nodes:
        return

    # Print root without prefix
    print(f"({nodes[0][0]})")

    # Print children
    for i, child_idx in enumerate(nodes[0][1]):
        is_last: bool = i == len(nodes[0][1]) - 1
        lines = build_tree(nodes, child_idx, "", is_last)
        for line in lines:
            print(line)


if __name__ == "__main__":
    try:
        input_string: str = sys.stdin.read().strip()
        print_tree(input_string)
    except Exception as e:
        print(f"Error: {str(e)}", file=sys.stderr)
        sys.exit(1)
