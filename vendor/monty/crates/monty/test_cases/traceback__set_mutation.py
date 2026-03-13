s = {1, 2}
for x in s:
    s.add(3)
"""
TRACEBACK:
Traceback (most recent call last):
  File "traceback__set_mutation.py", line 2, in <module>
    for x in s:
             ~
RuntimeError: Set changed size during iteration
"""
