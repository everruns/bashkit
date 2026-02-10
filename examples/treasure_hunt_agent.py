#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "bashkit[langchain]>=0.1.4",
#     "langchain>=1.0",
#     "langchain-anthropic>=0.3",
# ]
# ///
"""
Treasure Hunt Agent - A fun demonstration of LangChain + Bashkit

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

uv automatically installs bashkit from PyPI (pre-built wheels, no Rust needed).
"""

import asyncio
import os
import sys

from langchain.agents import create_agent

from bashkit.langchain import create_bash_tool


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


async def run_agent():
    """Run the treasure hunt agent with streaming output."""
    # Check for API key
    if not os.environ.get("ANTHROPIC_API_KEY"):
        print("Please set ANTHROPIC_API_KEY environment variable")
        print("  export ANTHROPIC_API_KEY=your_key_here")
        sys.exit(1)

    print("=" * 60)
    print("  TREASURE HUNT AGENT")
    print("  A LangChain + Bashkit Adventure")
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

    # Create the agent
    agent = create_agent(
        model="claude-sonnet-4-20250514",
        tools=[bash_tool],
        system_prompt=SYSTEM_PROMPT,
    )

    print("-" * 60)
    print("THE QUEST BEGINS!")
    print("-" * 60)

    # Stream events for real-time output
    async for event in agent.astream_events(
        {
            "messages": [
                {
                    "role": "user",
                    "content": "Begin your treasure hunt! Start at /home/agent/quest/START_HERE.txt "
                    "and follow the clues to find the treasure. Narrate your journey!",
                }
            ]
        },
        version="v2",
        config={"recursion_limit": 50},
    ):
        kind = event["event"]

        # Tool invocation
        if kind == "on_tool_start":
            cmd = event["data"].get("input", {}).get("commands", "")
            print(f"\n> Bash {cmd}")

        # Tool result
        elif kind == "on_tool_end":
            output = event["data"].get("output", "")
            # Handle ToolMessage or string
            if hasattr(output, "content"):
                output = output.content
            if output:
                lines = str(output).strip().split("\n")
                for line in lines[:10]:
                    print(f"  {line}")
                if len(lines) > 10:
                    print(f"  ... ({len(lines) - 10} more lines)")

        # Agent text output (streaming)
        elif kind == "on_chat_model_stream":
            chunk = event["data"].get("chunk")
            if chunk and hasattr(chunk, "content") and chunk.content:
                content = chunk.content
                if isinstance(content, str):
                    print(content, end="", flush=True)
                elif isinstance(content, list):
                    for block in content:
                        if isinstance(block, dict) and block.get("type") == "text":
                            print(block.get("text", ""), end="", flush=True)

    print()
    print("=" * 60)
    print("  QUEST COMPLETE!")
    print("=" * 60)


if __name__ == "__main__":
    asyncio.run(run_agent())
