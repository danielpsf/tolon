-- Lossy backfill, for pre-release rows only: V1 rows have no meeting start, so they get `at`.
-- Nothing shipped on V1. For a lead alarm the real start is later than `at`.
alter table alarms add column start text;
update alarms set start = at;
