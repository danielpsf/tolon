-- A name the person typed. Null means the feed's name (`name`, from X-WR-CALNAME or the host) shows.
alter table calendars add column custom_name text;
