if False:
    pass
elif False:
    pass
else:
    raise ValueError('in else body')
"""
TRACEBACK:
Traceback (most recent call last):
  File "if__raise_else.py", line 6, in <module>
    raise ValueError('in else body')
ValueError: in else body
"""
