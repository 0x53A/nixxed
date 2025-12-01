## General UI

Search bar at the top, three columns for Programs, Services, Packages below it. Below that, two lines, with help and status.

This means the minimum practical window height is 3 (search) + 3 (header, 1 line of content, box end) + 2 (help and status) = 8 lines.

When using the property editor, we need even more lines.

## Movement

You can either click on an item, or move with the arrow keys.

## Scroll Behavior

This should apply to all scrollbars in the application.

If we have 5 or more lines available, use decorators. In either case, leave at least one empty space available.

So, if we have 1 or 2 lines, well, can't help it, make the thumb 1 character. 3 or 4 lines, no decorators, scroll thumb is 1 character. 5 or more lines, add decorators, make scroll thumb size proportional, but make it at most n_lines-4 (two for the indicators, two for the empty spaces).

Where possible, moving up/and down should keep the viewport and move the selected item. When nearing the edge, keep one "lookahead", that is, start moving the viewport when we are one away from the edge, not just directly at the edge.

Move the thumb one line away from the scrollbar edge as soon as we are offset by a single item, so that the user has a visual indication that there is more to scroll. (Obvs only works for n_lines >= 3).


## Search behavior

We are using a web search. To not bombard the API, all search queries are cached, and the cache is used preferentially if the entry is less than a week old.