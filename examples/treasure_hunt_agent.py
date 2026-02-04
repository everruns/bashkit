#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "langchain-core>=0.3",
#     "langchain-anthropic>=0.3",
#     "bashkit-py",
# ]
# ///
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
- Decodes secrets with built-in tools

Run with:
    export ANTHROPIC_API_KEY=your_key
    uv run examples/treasure_hunt_agent.py

Or if bashkit-py is installed locally:
    cd python && maturin develop
    python examples/treasure_hunt_agent.py
"""

import asyncio
import os
import sys

from langchain_anthropic import ChatAnthropic
from langchain_core.messages import HumanMessage, SystemMessage
from langchain_core.prompts import ChatPromptTemplate, MessagesPlaceholder

# Try to import from installed package, fall back to local
try:
    from bashkit_py.langchain import create_bash_tool
except ImportError:
    print("bashkit-py not found. Install with: cd python && maturin develop")
    sys.exit(1)


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


async def main():
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

    # Create the LLM
    llm = ChatAnthropic(
        model="claude-sonnet-4-20250514",
        temperature=0.3,
    )

    # Bind the tool to the LLM
    llm_with_tools = llm.bind_tools([bash_tool])

    # System prompt for the treasure hunter
    system_prompt = """You are a brave adventurer on a treasure hunt!

You have access to a bash tool that lets you explore a virtual filesystem.
Your quest: Find the hidden treasure by following clues.

Available commands you might need:
- ls: List directory contents
- cat: Read file contents
- grep: Search for patterns in files
- find: Locate files by name
- cd: Change directory (use full paths)
- echo: Output text (useful for writing answers)

Strategy tips:
- Start by reading the START_HERE.txt file
- Follow each clue carefully
- Use grep to search for keywords
- Explore directories with ls
- Read promising files with cat

Be methodical and narrate your adventure as you go!"""

    messages = [
        SystemMessage(content=system_prompt),
        HumanMessage(
            content="Begin your treasure hunt! Start at /home/agent/quest/START_HERE.txt and follow the clues to find the treasure. Narrate your journey!"
        ),
    ]

    print("-" * 60)
    print("THE QUEST BEGINS!")
    print("-" * 60)
    print()

    # Agentic loop - let the agent explore
    max_iterations = 15
    iteration = 0

    while iteration < max_iterations:
        iteration += 1
        print(f"[Turn {iteration}]")

        # Get response from LLM
        response = await llm_with_tools.ainvoke(messages)
        messages.append(response)

        # Print the agent's thoughts
        if response.content:
            if isinstance(response.content, str):
                print(f"Agent: {response.content}")
            elif isinstance(response.content, list):
                for block in response.content:
                    if isinstance(block, dict) and block.get("type") == "text":
                        print(f"Agent: {block.get('text', '')}")

        # Check for tool calls
        if not response.tool_calls:
            print("\n[Agent finished - no more tool calls]")
            break

        # Execute each tool call
        for tool_call in response.tool_calls:
            tool_name = tool_call["name"]
            tool_args = tool_call["args"]

            print(f"\n  >> Executing: {tool_name}")
            if "commands" in tool_args:
                # Show abbreviated command
                cmd = tool_args["commands"]
                if len(cmd) > 80:
                    print(f"     Commands: {cmd[:77]}...")
                else:
                    print(f"     Commands: {cmd}")

            # Execute the tool
            try:
                result = bash_tool.invoke(tool_args)
                print(f"     Output: {result[:200]}..." if len(result) > 200 else f"     Output: {result}")
            except Exception as e:
                result = f"Error: {e}"
                print(f"     Error: {e}")

            # Add tool result to messages
            from langchain_core.messages import ToolMessage

            messages.append(
                ToolMessage(
                    content=result,
                    tool_call_id=tool_call["id"],
                )
            )

        print()

        # Check if treasure was found
        if "CONGRATULATIONS" in str(messages[-1].content):
            print("\n" + "=" * 60)
            print("  QUEST COMPLETE!")
            print("=" * 60)
            break

    if iteration >= max_iterations:
        print("\n[Max iterations reached]")

    print(f"\nTotal turns: {iteration}")


if __name__ == "__main__":
    asyncio.run(main())
