#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "langchain>=1.0",
#     "langchain-anthropic>=0.3",
# ]
# ///
# Note: Install bashkit first: cd crates/bashkit-python && maturin develop
"""
Treasure Hunt Agent - A fun demonstration of LangChain + BashKit

This agent plays a treasure hunt game where it must:
1. Set up a virtual filesystem with hidden clues
2. Follow the trail of clues using bash commands
3. Discover the final treasure

Demonstrates multiple agentic loop iterations as the agent:
- Reads clues with `cat`
- Searches files with `grep`
- Explores directories with `ls` and `find`
- Solves a riddle

Run with:
    export ANTHROPIC_API_KEY=your_key
    uv run examples/treasure_hunt_agent.py
"""

import os
import sys
from typing import Any

from langchain.agents import create_agent
from langchain_core.callbacks import BaseCallbackHandler

# Try to import from installed package
try:
    from bashkit.langchain import create_bash_tool
except ImportError:
    print("bashkit not found. Install with: cd crates/bashkit-python && maturin develop")
    sys.exit(1)


class ToolVisualizerCallback(BaseCallbackHandler):
    """Callback handler to visualize tool invocations."""

    def __init__(self):
        self.tool_count = 0

    def on_tool_start(
        self, serialized: dict[str, Any], input_str: str, **kwargs: Any
    ) -> None:
        """Called when a tool starts running."""
        self.tool_count += 1
        tool_name = serialized.get("name", "unknown")
        print(f"\n{'='*60}")
        print(f"  TOOL CALL #{self.tool_count}: {tool_name}")
        print(f"{'='*60}")
        # Extract command from input
        if isinstance(input_str, str):
            # Try to parse as dict repr
            if input_str.startswith("{"):
                import ast
                try:
                    d = ast.literal_eval(input_str)
                    cmd = d.get("commands", input_str)
                except Exception:
                    cmd = input_str
            else:
                cmd = input_str
        else:
            cmd = str(input_str)
        print(f"  $ {cmd}")
        print()

    def on_tool_end(self, output: Any, **kwargs: Any) -> None:
        """Called when a tool finishes."""
        # Handle different output types
        if hasattr(output, "content"):
            text = output.content
        elif isinstance(output, str):
            text = output
        else:
            text = str(output)

        print(f"  Output:")
        lines = text.strip().split("\n")
        for line in lines[:15]:
            print(f"    {line}")
        if len(lines) > 15:
            print(f"    ... ({len(lines) - 15} more lines)")
        print(f"{'='*60}")


# The treasure hunt setup script - creates clues in the virtual filesystem
TREASURE_HUNT_SETUP = '''
# Create the treasure hunt!
mkdir -p /home/agent/quest
mkdir -p /home/agent/quest/forest
mkdir -p /home/agent/quest/cave
mkdir -p /home/agent/quest/castle

# First clue - in the starting area
cat > /home/agent/quest/START_HERE.txt << 'EOF'
Welcome, brave adventurer!

Your quest begins now. Hidden somewhere in this virtual realm
is a legendary treasure. Follow the clues to find it!

FIRST CLUE: The forest holds secrets. Look for something that
speaks of ancient trees in the forest directory.
EOF

# Second clue - in the forest
cat > /home/agent/quest/forest/ancient_trees.txt << 'EOF'
You found the Forest of Whispers!

The ancient trees tell of a hidden cave where shadows dance.
Seek the file that mentions "glowing" - it holds your next hint.

There are many files here... use your search skills wisely.
EOF

# Decoy files in forest
echo "Just some ordinary bushes here." > /home/agent/quest/forest/bushes.txt
echo "A babbling brook flows through." > /home/agent/quest/forest/stream.txt
echo "Birds sing in the canopy." > /home/agent/quest/forest/birds.txt

# Third clue - also in forest (need to grep for it)
cat > /home/agent/quest/forest/mysterious_light.txt << 'EOF'
Among the shadows, you spot a glowing mushroom!

It pulses with an ethereal light and shows you a vision:
"The cave entrance lies to the east. Inside, find the
file containing the riddle of THREE."

Remember: The answer is always hidden in plain sight.
EOF

# Fourth clue - in the cave
cat > /home/agent/quest/cave/riddle.txt << 'EOF'
You enter the dark cave...

THE RIDDLE OF THREE:
I have cities, but no houses.
I have mountains, but no trees.
I have water, but no fish.
What am I?

Write the answer (lowercase, no spaces) to the file
/home/agent/quest/cave/answer.txt

Then look in the castle for files modified most recently.
The treasure awaits those who solve the riddle!
EOF

# Decoy in cave
echo "Bats flutter overhead." > /home/agent/quest/cave/bats.txt
echo "Stalactites drip slowly." > /home/agent/quest/cave/stalactites.txt

# Fifth clue - in the castle (treasure location)
cat > /home/agent/quest/castle/throne_room.txt << 'EOF'
The Castle of Victories!

You have proven yourself worthy, adventurer.
The treasure chest awaits in this very room...

To claim your prize, read the TREASURE.txt file.
But first, you must have solved the cave riddle!
EOF

# The treasure itself
cat > /home/agent/quest/castle/TREASURE.txt << 'EOF'

    *************************************
    *                                   *
    *   CONGRATULATIONS, ADVENTURER!    *
    *                                   *
    *   You found the legendary         *
    *   GOLDEN CODE OF WISDOM!          *
    *                                   *
    *   "A program is never finished,   *
    *    only released." - Anonymous    *
    *                                   *
    *   Your reward: 1000 virtual gold  *
    *   and eternal glory!              *
    *                                   *
    *************************************

Quest completed! You used your bash skills masterfully:
- Explored directories with ls
- Read clues with cat
- Searched files with grep
- Solved riddles with wit

The answer to the riddle was: map
(A map has cities, mountains, and water, but none of them are real!)
EOF

echo "Treasure hunt created! Start at /home/agent/quest/START_HERE.txt"
'''

SYSTEM_PROMPT = """You are a brave adventurer on a treasure hunt!

You have access to a bash tool that lets you explore a virtual filesystem.
Your quest: Find the hidden treasure by following clues.

Available commands you might need:
- ls: List directory contents
- cat: Read file contents
- grep: Search for patterns in files (use: grep "pattern" file or grep -r "pattern" dir/*)
- find: Locate files by name
- echo: Output text (useful for writing answers)

Strategy tips:
- Start by reading the START_HERE.txt file
- Follow each clue carefully
- Use grep to search for keywords
- Explore directories with ls
- Read promising files with cat

Be methodical and narrate your adventure as you go!
When you find the treasure (CONGRATULATIONS message), summarize your journey and stop."""


def main():
    # Check for API key
    if not os.environ.get("ANTHROPIC_API_KEY"):
        print("Please set ANTHROPIC_API_KEY environment variable")
        print("  export ANTHROPIC_API_KEY=your_key_here")
        sys.exit(1)

    print("=" * 60)
    print("  TREASURE HUNT AGENT")
    print("  A LangChain + BashKit Adventure")
    print("=" * 60)
    print()

    # Create the bash tool
    bash_tool = create_bash_tool(
        username="agent",
        hostname="questworld",
        max_commands=500,
    )

    # Set up the treasure hunt in the virtual filesystem
    print("Setting up the treasure hunt...")
    setup_result = bash_tool.invoke({"commands": TREASURE_HUNT_SETUP})
    print(f"Setup: {setup_result.strip()}")
    print()

    # Create the agent using LangChain v1's create_agent
    agent = create_agent(
        model="claude-sonnet-4-20250514",
        tools=[bash_tool],
        system_prompt=SYSTEM_PROMPT,
    )

    print("-" * 60)
    print("THE QUEST BEGINS!")
    print("-" * 60)
    print()

    # Create callback for tool visualization
    callback = ToolVisualizerCallback()

    # Run the agent with callbacks
    result = agent.invoke(
        {
            "messages": [
                {
                    "role": "user",
                    "content": "Begin your treasure hunt! Start at /home/agent/quest/START_HERE.txt "
                    "and follow the clues to find the treasure. Narrate your journey!",
                }
            ]
        },
        config={"callbacks": [callback]},
    )

    # Print the final response
    if "messages" in result:
        for msg in result["messages"]:
            if hasattr(msg, "content") and msg.content:
                content = msg.content
                if isinstance(content, str) and content.strip():
                    print(content)
                elif isinstance(content, list):
                    for block in content:
                        if isinstance(block, dict) and block.get("type") == "text":
                            text = block.get("text", "")
                            if text.strip():
                                print(text)

    # Print final message
    print()
    print("=" * 60)
    print("  QUEST COMPLETE!")
    print("=" * 60)


if __name__ == "__main__":
    main()
