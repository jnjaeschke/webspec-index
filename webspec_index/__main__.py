"""CLI entry point using Click"""

import click
import json
import sys
from . import (
    query,
    search,
    exists,
    anchors,
    list_headings,
    refs,
    update,
    spec_urls,
    clear_db,
    __version__,
)


@click.group()
@click.version_option(version=__version__)
@click.option(
    "--format",
    type=click.Choice(["json", "markdown"]),
    default="json",
    help="Output format",
)
@click.pass_context
def cli(ctx, format):
    """webspec-index: Query WHATWG/W3C web specifications

    Examples:
      webspec-index query HTML#navigate
      webspec-index search "tree order" --spec DOM
      webspec-index mcp  # Start MCP server for AI agents
    """
    ctx.ensure_object(dict)
    ctx.obj["format"] = format


@cli.command()
@click.argument("spec_anchor")
@click.option("--sha", help="Specific commit SHA to query")
@click.pass_context
def query_cmd(ctx, spec_anchor, sha):
    """Query a specific section in a spec

    SPEC_ANCHOR format: SPEC#anchor (e.g., HTML#navigate)
    """
    fmt = ctx.obj["format"]
    try:
        result = query(spec_anchor, sha)
        if fmt == "json":
            click.echo(json.dumps(result, indent=2))
        else:
            click.echo(result.get("content", ""))
    except Exception as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)


@cli.command()
@click.argument("query_text")
@click.option("--spec", help="Limit search to specific spec")
@click.option("--limit", type=int, default=20, help="Maximum results")
@click.pass_context
def search_cmd(ctx, query_text, spec, limit):
    """Search for text across all specs"""
    fmt = ctx.obj["format"]
    try:
        result = search(query_text, spec, limit)
        if fmt == "json":
            click.echo(json.dumps(result, indent=2))
        else:
            for entry in result.get("results", []):
                click.echo(entry.get("snippet", ""))
    except Exception as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)


@cli.command()
@click.argument("spec_anchor")
def exists_cmd(spec_anchor):
    """Check if a section exists"""
    try:
        result = exists(spec_anchor)
        click.echo("true" if result else "false")
        sys.exit(0 if result else 1)
    except Exception as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)


@cli.command()
@click.argument("pattern")
@click.option("--spec", help="Limit search to specific spec")
@click.option("--limit", type=int, default=50, help="Maximum results")
def anchors_cmd(pattern, spec, limit):
    """Find anchors matching a pattern"""
    try:
        results = anchors(pattern, spec, limit)
        for anchor in results:
            click.echo(f"{anchor['spec']}#{anchor['anchor']}\t{anchor['title'] or ''}")
    except Exception as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)


@cli.command()
@click.argument("spec")
@click.option("--sha", help="Specific commit SHA to query")
@click.pass_context
def list_cmd(ctx, spec, sha):
    """List all headings in a spec"""
    fmt = ctx.obj["format"]
    try:
        headings = list_headings(spec, sha)
        if fmt == "json":
            click.echo(json.dumps(headings, indent=2))
        else:
            # TODO: Implement markdown tree formatting
            for h in headings:
                indent = "  " * h["depth"]
                title = h["title"] or h["anchor"]
                click.echo(f"{indent}{title} (#{h['anchor']})")
    except Exception as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)


@cli.command()
@click.argument("spec_anchor")
@click.option(
    "--direction", type=click.Choice(["incoming", "outgoing", "both"]), default="both"
)
@click.option("--sha", help="Specific commit SHA to query")
@click.pass_context
def refs_cmd(ctx, spec_anchor, direction, sha):
    """Get references for a section"""
    fmt = ctx.obj["format"]
    try:
        result = refs(spec_anchor, direction, sha)
        if fmt == "json":
            click.echo(json.dumps(result, indent=2))
        else:
            for r in result.get("outgoing", []) + result.get("incoming", []):
                click.echo(f"{r['spec']}#{r['anchor']}")
    except Exception as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)


@cli.command()
@click.option("--spec", help="Update specific spec (updates all if not specified)")
@click.option("--force", is_flag=True, help="Force update even if recently checked")
def update_cmd(spec, force):
    """Update specs (fetch latest versions)"""
    try:
        result = update(spec, force)
        for spec_name, snapshot_id in result:
            if snapshot_id:
                click.echo(f"Updated {spec_name} (snapshot {snapshot_id})")
            else:
                click.echo(f"{spec_name} already up to date")
    except Exception as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)


@cli.command()
@click.option("--yes", is_flag=True, help="Skip confirmation prompt")
def clear_db_cmd(yes):
    """Clear the database (removes all indexed data)"""
    if not yes:
        click.confirm("This will delete all indexed spec data. Continue?", abort=True)

    try:
        path = clear_db()
        click.echo(f"Database cleared: {path}")
    except Exception as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)


@cli.command()
def specs():
    """List all known spec base URLs"""
    for entry in spec_urls():
        click.echo(f"{entry['spec']}\t{entry['base_url']}")


@cli.command()
@click.option("--stdio", is_flag=True, default=False, hidden=True,
              help="Use stdio transport (default, accepted for LSP client compatibility).")
def lsp(**_kwargs):
    """Start Language Server Protocol server

    Communicates over stdio. Used by editor extensions.

    Installation:
      # VSCode: install the spec-lens extension
      # Neovim: vim.lsp.start({ cmd = { "webspec-index", "lsp" } })
    """
    try:
        from .lsp import start_server
        start_server()
    except ImportError as e:
        click.echo(
            "LSP dependencies not installed. Install with:\n"
            "  pip install 'webspec-index[lsp]'\n"
            f"\nMissing: {e}",
            err=True,
        )
        sys.exit(1)


@cli.command()
def mcp():
    """Start MCP server for AI agents

    This starts a Model Context Protocol server that communicates over stdio.
    AI agents can connect to this server to query web specifications.

    Installation:
      uvx webspec-index mcp
      # or: claude mcp add uvx webspec-index
    """
    try:
        import asyncio
        from .mcp import run_server

        asyncio.run(run_server())
    except KeyboardInterrupt:
        click.echo("\nMCP server stopped", err=True)
    except Exception as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)


# Rename commands to match expected names
cli.add_command(query_cmd, name="query")
cli.add_command(search_cmd, name="search")
cli.add_command(exists_cmd, name="exists")
cli.add_command(anchors_cmd, name="anchors")
cli.add_command(list_cmd, name="list")
cli.add_command(refs_cmd, name="refs")
cli.add_command(update_cmd, name="update")
cli.add_command(clear_db_cmd, name="clear-db")


if __name__ == "__main__":
    cli()
