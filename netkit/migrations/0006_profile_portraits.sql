CREATE TABLE IF NOT EXISTS public_profile_portraits(
    home TEXT NOT NULL,
    owner TEXT NOT NULL,
    portrait TEXT NOT NULL,
    PRIMARY KEY(home,owner)
);
CREATE INDEX IF NOT EXISTS public_profile_portraits_owner ON public_profile_portraits(owner);
