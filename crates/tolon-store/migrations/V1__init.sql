create table calendars (id text primary key, kind text not null, name text not null, enabled integer not null default 1,
  url text, account text, color text, etag text, last_sync_at text, last_error text, created_at text not null);
create table events (id text primary key, calendar_id text not null references calendars(id) on delete cascade,
  uid text not null, payload text not null, updated_at text not null, unique(calendar_id, uid));
create table alarms (occurrence_key text not null, kind text not null, at text not null, state text not null,
  state_at text not null, until text, primary key(occurrence_key, kind));
create table settings (key text primary key, value text not null);
create table sync_log (id integer primary key, calendar_id text, at text not null, outcome text not null, detail text);
