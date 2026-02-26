"""MCP server for AI agent integration"""

import json
import logging
from mcp.server import Server
from mcp.types import Tool, TextContent, CallToolResult
from . import query, search, exists, anchors, list_headings, refs, update

# Configure logging
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger("webspec-index-mcp")

# Create server instance
server = Server("webspec-index")


@server.list_tools()
async def list_tools() -> list[Tool]:
    """List available tools for AI agents"""
    return [
        Tool(
            name="query_spec",
            description="Query a specific section in a web specification. Returns section info, children, and cross-references.",
            inputSchema={
                "type": "object",
                "properties": {
                    "spec_anchor": {
                        "type": "string",
                        "description": "Spec and anchor in format 'SPEC#anchor' (e.g., 'HTML#navigate', 'DOM#concept-tree')"
                    }
                },
                "required": ["spec_anchor"]
            }
        ),
        Tool(
            name="search_specs",
            description="Search for text across all web specifications using full-text search.",
            inputSchema={
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Text to search for"
                    },
                    "spec": {
                        "type": "string",
                        "description": "Optional spec name to limit search (e.g., 'HTML', 'DOM')"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default 20)",
                        "default": 20
                    }
                },
                "required": ["query"]
            }
        ),
        Tool(
            name="check_exists",
            description="Check if a section exists in a web specification.",
            inputSchema={
                "type": "object",
                "properties": {
                    "spec_anchor": {
                        "type": "string",
                        "description": "Spec and anchor in format 'SPEC#anchor'"
                    }
                },
                "required": ["spec_anchor"]
            }
        ),
        Tool(
            name="find_anchors",
            description="Find anchors matching a glob pattern in web specifications.",
            inputSchema={
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Glob pattern (e.g., '*-tree', 'concept-*')"
                    },
                    "spec": {
                        "type": "string",
                        "description": "Optional spec name to limit search"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default 50)",
                        "default": 50
                    }
                },
                "required": ["pattern"]
            }
        ),
        Tool(
            name="list_headings",
            description="List all headings in a web specification.",
            inputSchema={
                "type": "object",
                "properties": {
                    "spec": {
                        "type": "string",
                        "description": "Spec name (e.g., 'HTML', 'DOM')"
                    }
                },
                "required": ["spec"]
            }
        ),
        Tool(
            name="get_references",
            description="Get cross-references for a section (incoming, outgoing, or both).",
            inputSchema={
                "type": "object",
                "properties": {
                    "spec_anchor": {
                        "type": "string",
                        "description": "Spec and anchor in format 'SPEC#anchor'"
                    },
                    "direction": {
                        "type": "string",
                        "enum": ["incoming", "outgoing", "both"],
                        "description": "Direction of references to return",
                        "default": "both"
                    }
                },
                "required": ["spec_anchor"]
            }
        ),
        Tool(
            name="update_specs",
            description="Update web specifications to latest versions from WHATWG/W3C.",
            inputSchema={
                "type": "object",
                "properties": {
                    "spec": {
                        "type": "string",
                        "description": "Optional spec name to update (updates all if not specified)"
                    },
                    "force": {
                        "type": "boolean",
                        "description": "Force update even if recently checked",
                        "default": False
                    }
                }
            }
        )
    ]


@server.call_tool()
async def call_tool(name: str, arguments: dict) -> CallToolResult:
    """Handle tool calls from AI agents"""
    try:
        if name == "query_spec":
            result = query(arguments["spec_anchor"])
            return CallToolResult(
                content=[TextContent(
                    type="text",
                    text=json.dumps(result, indent=2)
                )]
            )

        elif name == "search_specs":
            result = search(
                arguments["query"],
                arguments.get("spec"),
                arguments.get("limit", 20)
            )
            return CallToolResult(
                content=[TextContent(
                    type="text",
                    text=json.dumps(result, indent=2)
                )]
            )

        elif name == "check_exists":
            result = exists(arguments["spec_anchor"])
            return CallToolResult(
                content=[TextContent(
                    type="text",
                    text=json.dumps({"exists": result})
                )]
            )

        elif name == "find_anchors":
            result = anchors(
                arguments["pattern"],
                arguments.get("spec"),
                arguments.get("limit", 50)
            )
            return CallToolResult(
                content=[TextContent(
                    type="text",
                    text=json.dumps({"results": result}, indent=2)
                )]
            )

        elif name == "list_headings":
            result = list_headings(arguments["spec"])
            return CallToolResult(
                content=[TextContent(
                    type="text",
                    text=json.dumps(result, indent=2)
                )]
            )

        elif name == "get_references":
            result = refs(
                arguments["spec_anchor"],
                arguments.get("direction", "both"),
            )
            return CallToolResult(
                content=[TextContent(
                    type="text",
                    text=json.dumps(result, indent=2)
                )]
            )

        elif name == "update_specs":
            result = update(arguments.get("spec"), arguments.get("force", False))
            return CallToolResult(
                content=[TextContent(
                    type="text",
                    text=json.dumps(result, indent=2)
                )]
            )

        else:
            return CallToolResult(
                content=[TextContent(
                    type="text",
                    text=json.dumps({"error": f"Unknown tool: {name}"})
                )],
                isError=True
            )

    except Exception as e:
        logger.error(f"Tool call error: {e}")
        return CallToolResult(
            content=[TextContent(
                type="text",
                text=json.dumps({"error": str(e)})
            )],
            isError=True
        )


async def run_server():
    """Run the MCP server over stdio"""
    from mcp.server.stdio import stdio_server

    logger.info("Starting webspec-index MCP server")
    async with stdio_server() as (read_stream, write_stream):
        await server.run(
            read_stream,
            write_stream,
            server.create_initialization_options()
        )
