"""CLI entry point using Click"""

import click
import json
import sys
from . import query, search, exists, anchors, list_headings, refs, update, clear_db, __version__


@click.group()
@click.version_option(version=__version__)
def cli():
    """webspec-index: Query WHATWG/W3C web specifications

    Examples:
      webspec-index query HTML#navigate
      webspec-index search "tree order" --spec DOM
      webspec-index mcp  # Start MCP server for AI agents
    """
    pass


@cli.command()
@click.argument("spec_anchor")
@click.option("--sha", help="Specific commit SHA to query")
@click.option("--format", type=click.Choice(["json", "markdown"]), default="json")
def query_cmd(spec_anchor, sha, format):
    """Query a specific section in a spec

    SPEC_ANCHOR format: SPEC#anchor (e.g., HTML#navigate)
    """
    try:
        result = query(spec_anchor, sha)
        if format == "json":
            click.echo(json.dumps(result, indent=2))
        else:
            # TODO: Implement markdown formatting
            click.echo(json.dumps(result, indent=2))
    except Exception as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)


@cli.command()
@click.argument("query_text")
@click.option("--spec", help="Limit search to specific spec")
@click.option("--limit", type=int, default=20, help="Maximum results")
@click.option("--format", type=click.Choice(["json", "markdown"]), default="json")
def search_cmd(query_text, spec, limit, format):
    """Search for text across all specs"""
    try:
        result = search(query_text, spec, limit)
        if format == "json":
            click.echo(json.dumps(result, indent=2))
        else:
            # TODO: Implement markdown formatting
            click.echo(json.dumps(result, indent=2))
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
@click.option("--format", type=click.Choice(["json", "markdown"]), default="json")
def list_cmd(spec, sha, format):
    """List all headings in a spec"""
    try:
        headings = list_headings(spec, sha)
        if format == "json":
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
@click.option("--direction", type=click.Choice(["incoming", "outgoing", "both"]), default="both")
@click.option("--sha", help="Specific commit SHA to query")
@click.option("--format", type=click.Choice(["json", "markdown"]), default="json")
def refs_cmd(spec_anchor, direction, sha, format):
    """Get references for a section"""
    try:
        result = refs(spec_anchor, direction, sha)
        if format == "json":
            click.echo(json.dumps(result, indent=2))
        else:
            # TODO: Implement markdown formatting
            click.echo(json.dumps(result, indent=2))
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
