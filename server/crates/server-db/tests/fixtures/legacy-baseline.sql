-- Test fixture: the schema that the inherited Prisma migration chain
-- (server/prisma/migrations, 121 migrations) produces, written as plain SQL.
--
-- This file is NOT a product migration and is never embedded in the server.
-- It exists only so that the test target server_db_legacy_schema can prove,
-- on every supported PostgreSQL major version, that this plain SQL and the
-- inherited chain create the same schema, including constraint names.
--
-- Origin: pg_dump --schema-only --no-owner --no-privileges of the schema after
-- the chain, without _prisma_migrations, run on PostgreSQL 17. PostgreSQL 18
-- records NOT NULL constraints as named catalog entries; the historical names
-- that the chain leaves behind after column renames are therefore written out
-- (PostgreSQL 16 and 17 accept and ignore them). The file has no psql
-- meta-commands, so it runs through sqlx::raw_sql.

--
--

--
-- Name: pg_trgm; Type: EXTENSION; Schema: -; Owner: -
--

CREATE EXTENSION IF NOT EXISTS pg_trgm WITH SCHEMA public;

--
-- Name: APITokenMode; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public."APITokenMode" AS ENUM (
    'User',
    'System',
    'Client'
);

--
-- Name: AgeRatingOrganization; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public."AgeRatingOrganization" AS ENUM (
    'ESRB',
    'PEGI',
    'CERO',
    'USK',
    'GRAC',
    'ClassInd',
    'ACB'
);

--
-- Name: AuthMec; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public."AuthMec" AS ENUM (
    'Simple',
    'OpenID'
);

--
-- Name: ClientCapabilities; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public."ClientCapabilities" AS ENUM (
    'peerAPI',
    'userStatus',
    'cloudSaves',
    'trackPlaytime'
);

--
-- Name: GameType; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public."GameType" AS ENUM (
    'Game',
    'Emulator',
    'Dependency'
);

--
-- Name: LibraryBackend; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public."LibraryBackend" AS ENUM (
    'Filesystem',
    'FlatFilesystem'
);

--
-- Name: MFAMec; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public."MFAMec" AS ENUM (
    'WebAuthn',
    'TOTP'
);

--
-- Name: MetadataSource; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public."MetadataSource" AS ENUM (
    'Manual',
    'GiantBomb',
    'PCGamingWiki',
    'IGDB',
    'Metacritic',
    'OpenCritic',
    'Steam'
);

--
-- Name: Platform; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public."Platform" AS ENUM (
    'windows',
    'linux',
    'macos'
);

--
-- Name: APIToken; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."APIToken" (
    token text NOT NULL,
    mode public."APITokenMode" NOT NULL,
    "userId" text,
    acls text[],
    name text NOT NULL,
    id text NOT NULL,
    "clientId" text,
    "expiresAt" timestamp(3) without time zone
);

--
-- Name: ApplicationSettings; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."ApplicationSettings" (
    "timestamp" timestamp(3) without time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    "metadataProviders" text[],
    "saveSlotCountLimit" integer DEFAULT 5 NOT NULL,
    "saveSlotSizeLimit" double precision DEFAULT 10 NOT NULL,
    "saveSlotHistoryLimit" integer DEFAULT 3 NOT NULL,
    "showGamePanelTextDecoration" boolean DEFAULT true NOT NULL,
    "mLogoObjectId" text,
    "serverName" text DEFAULT 'Drop'::text NOT NULL
);

--
-- Name: Article; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."Article" (
    id text NOT NULL,
    title text NOT NULL,
    description text NOT NULL,
    content text NOT NULL,
    "imageObjectId" text,
    "publishedAt" timestamp(3) without time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    "authorId" text
);

--
-- Name: BannedAgeRating; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."BannedAgeRating" (
    id text NOT NULL,
    organization public."AgeRatingOrganization" NOT NULL,
    rating text NOT NULL,
    "userGroupId" text NOT NULL
);

--
-- Name: Certificate; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."Certificate" (
    id text NOT NULL,
    "privateKey" text NOT NULL,
    certificate text NOT NULL,
    blacklisted boolean DEFAULT false NOT NULL
);

--
-- Name: Client; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."Client" (
    "userId" text NOT NULL,
    capabilities public."ClientCapabilities"[],
    id text NOT NULL,
    "lastConnected" timestamp(3) without time zone NOT NULL,
    name text NOT NULL,
    platform public."Platform" NOT NULL
);

--
-- Name: Collection; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."Collection" (
    id text NOT NULL,
    name text NOT NULL,
    "isDefault" boolean DEFAULT false NOT NULL,
    "userId" text NOT NULL
);

--
-- Name: CollectionEntry; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."CollectionEntry" (
    "collectionId" text NOT NULL,
    "gameId" text NOT NULL
);

--
-- Name: Company; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."Company" (
    id text NOT NULL,
    "metadataSource" public."MetadataSource" NOT NULL,
    "metadataId" text NOT NULL,
    "metadataOriginalQuery" text NOT NULL,
    "mName" text NOT NULL,
    "mShortDescription" text NOT NULL,
    "mDescription" text NOT NULL,
    "mLogoObjectId" text NOT NULL,
    "mBannerObjectId" text NOT NULL,
    "mWebsite" text NOT NULL
);

--
-- Name: Depot; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."Depot" (
    id text NOT NULL,
    endpoint text NOT NULL,
    key text NOT NULL
);

--
-- Name: Game; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."Game" (
    id text NOT NULL,
    "metadataSource" public."MetadataSource" NOT NULL,
    "metadataId" text NOT NULL,
    "mName" text NOT NULL,
    "mShortDescription" text NOT NULL,
    "mDescription" text NOT NULL,
    "mIconObjectId" text CONSTRAINT "Game_mIconId_not_null" NOT NULL,
    "libraryPath" text CONSTRAINT "Game_libraryBasePath_not_null" NOT NULL,
    "mImageLibraryObjectIds" text[],
    "mBannerObjectId" text CONSTRAINT "Game_mBannerId_not_null" NOT NULL,
    "mCoverObjectId" text CONSTRAINT "Game_mCoverId_not_null" NOT NULL,
    created timestamp(3) without time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    "mReleased" timestamp(3) without time zone NOT NULL,
    "mImageCarouselObjectIds" text[] DEFAULT ARRAY[]::text[],
    "libraryId" text NOT NULL,
    featured boolean DEFAULT false NOT NULL,
    type public."GameType" DEFAULT 'Game'::public."GameType" NOT NULL
);

--
-- Name: GameAgeRating; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."GameAgeRating" (
    id text NOT NULL,
    organization public."AgeRatingOrganization" NOT NULL,
    rating text NOT NULL,
    "ratingCoverUrl" text,
    "gameId" text NOT NULL
);

--
-- Name: GameRating; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."GameRating" (
    id text NOT NULL,
    "metadataSource" public."MetadataSource" NOT NULL,
    "metadataId" text NOT NULL,
    created timestamp(3) without time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    "mReviewCount" integer NOT NULL,
    "mReviewRating" double precision NOT NULL,
    "mReviewHref" text,
    "gameId" text NOT NULL
);

--
-- Name: GameTag; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."GameTag" (
    id text NOT NULL,
    name text NOT NULL
);

--
-- Name: GameVersion; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."GameVersion" (
    "gameId" text NOT NULL,
    "dropletManifest" jsonb NOT NULL,
    delta boolean DEFAULT false NOT NULL,
    "versionIndex" integer NOT NULL,
    created timestamp(3) without time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    "onlySetup" boolean DEFAULT false NOT NULL,
    "displayName" text,
    "versionId" text NOT NULL,
    "versionPath" text,
    "fileList" text[],
    "negativeFileList" text[]
);

--
-- Name: Invitation; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."Invitation" (
    id text NOT NULL,
    "isAdmin" boolean DEFAULT false NOT NULL,
    username text,
    email text,
    expires timestamp(3) without time zone NOT NULL
);

--
-- Name: LaunchConfiguration; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."LaunchConfiguration" (
    "launchId" text NOT NULL,
    name text NOT NULL,
    command text NOT NULL,
    platform public."Platform" NOT NULL,
    "umuIdOverride" text,
    "versionId" text NOT NULL,
    "umuStoreOverride" text,
    "emulatorId" text,
    "emulatorSuggestions" text[]
);

--
-- Name: Library; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."Library" (
    id text NOT NULL,
    name text NOT NULL,
    backend public."LibraryBackend" NOT NULL,
    options jsonb NOT NULL
);

--
-- Name: LinkedAuthMec; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."LinkedAuthMec" (
    "userId" text NOT NULL,
    mec public."AuthMec" NOT NULL,
    credentials jsonb NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    version integer DEFAULT 1 NOT NULL
);

--
-- Name: LinkedMFAMec; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."LinkedMFAMec" (
    "userId" text NOT NULL,
    mec public."MFAMec" NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    version integer DEFAULT 1 NOT NULL,
    credentials jsonb NOT NULL
);

--
-- Name: NewsTag; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."NewsTag" (
    id text NOT NULL,
    name text NOT NULL
);

--
-- Name: Notification; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."Notification" (
    id text NOT NULL,
    nonce text,
    "userId" text NOT NULL,
    title text NOT NULL,
    description text NOT NULL,
    actions text[],
    read boolean DEFAULT false NOT NULL,
    created timestamp(3) without time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    acls text[]
);

--
-- Name: ObjectHash; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."ObjectHash" (
    id text NOT NULL,
    hash text NOT NULL
);

--
-- Name: Playtime; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."Playtime" (
    "gameId" text NOT NULL,
    "userId" text NOT NULL,
    seconds integer NOT NULL,
    "updatedAt" timestamp(6) with time zone NOT NULL,
    "createdAt" timestamp(6) with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL
);

--
-- Name: SaveSlot; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."SaveSlot" (
    "gameId" text NOT NULL,
    "userId" text NOT NULL,
    index integer NOT NULL,
    "createdAt" timestamp(3) without time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    playtime double precision DEFAULT 0 NOT NULL,
    "lastUsedClientId" text,
    "historyObjectIds" text[],
    "historyChecksums" text[]
);

--
-- Name: Screenshot; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."Screenshot" (
    id text NOT NULL,
    "gameId" text NOT NULL,
    "userId" text NOT NULL,
    "objectId" text NOT NULL,
    private boolean NOT NULL,
    "createdAt" timestamp(0) with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL
);

--
-- Name: Session; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."Session" (
    token text NOT NULL,
    data jsonb NOT NULL,
    "expiresAt" timestamp(3) without time zone NOT NULL,
    "userId" text
);

--
-- Name: SetupConfiguration; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."SetupConfiguration" (
    "setupId" text NOT NULL,
    command text NOT NULL,
    platform public."Platform" NOT NULL,
    "versionId" text NOT NULL
);

--
-- Name: Task; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."Task" (
    id text NOT NULL,
    "taskGroup" text NOT NULL,
    started timestamp(3) without time zone NOT NULL,
    ended timestamp(3) without time zone NOT NULL,
    success boolean NOT NULL,
    error jsonb,
    progress double precision NOT NULL,
    log text[],
    acls text[],
    name text NOT NULL,
    actions text[]
);

--
-- Name: UnimportedGameVersion; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."UnimportedGameVersion" (
    id text NOT NULL,
    "gameId" text NOT NULL,
    "versionName" text NOT NULL,
    manifest jsonb NOT NULL,
    "fileList" text[]
);

--
-- Name: User; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."User" (
    id text NOT NULL,
    username text NOT NULL,
    admin boolean DEFAULT false NOT NULL,
    "displayName" text NOT NULL,
    email text NOT NULL,
    "profilePictureObjectId" text CONSTRAINT "User_profilePicture_not_null" NOT NULL,
    enabled boolean DEFAULT true NOT NULL
);

--
-- Name: UserGroup; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."UserGroup" (
    id text NOT NULL,
    name text NOT NULL,
    description text DEFAULT ''::text NOT NULL,
    "createdAt" timestamp(3) without time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    "updatedAt" timestamp(3) without time zone NOT NULL
);

--
-- Name: _ArticleToNewsTag; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."_ArticleToNewsTag" (
    "A" text NOT NULL,
    "B" text NOT NULL
);

--
-- Name: _GameToGameTag; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."_GameToGameTag" (
    "A" text NOT NULL,
    "B" text NOT NULL
);

--
-- Name: _UserToUserGroup; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."_UserToUserGroup" (
    "A" text NOT NULL,
    "B" text NOT NULL
);

--
-- Name: _developers; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public._developers (
    "A" text NOT NULL,
    "B" text NOT NULL
);

--
-- Name: _publishers; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public._publishers (
    "A" text NOT NULL,
    "B" text NOT NULL
);

--
-- Name: _requiredContent; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."_requiredContent" (
    "A" text NOT NULL,
    "B" text NOT NULL
);

--
-- Name: APIToken APIToken_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."APIToken"
    ADD CONSTRAINT "APIToken_pkey" PRIMARY KEY (id);

--
-- Name: ApplicationSettings ApplicationSettings_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."ApplicationSettings"
    ADD CONSTRAINT "ApplicationSettings_pkey" PRIMARY KEY ("timestamp");

--
-- Name: Article Article_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Article"
    ADD CONSTRAINT "Article_pkey" PRIMARY KEY (id);

--
-- Name: BannedAgeRating BannedAgeRating_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."BannedAgeRating"
    ADD CONSTRAINT "BannedAgeRating_pkey" PRIMARY KEY (id);

--
-- Name: Certificate Certificate_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Certificate"
    ADD CONSTRAINT "Certificate_pkey" PRIMARY KEY (id);

--
-- Name: Client Client_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Client"
    ADD CONSTRAINT "Client_pkey" PRIMARY KEY (id);

--
-- Name: CollectionEntry CollectionEntry_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."CollectionEntry"
    ADD CONSTRAINT "CollectionEntry_pkey" PRIMARY KEY ("collectionId", "gameId");

--
-- Name: Collection Collection_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Collection"
    ADD CONSTRAINT "Collection_pkey" PRIMARY KEY (id);

--
-- Name: Company Company_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Company"
    ADD CONSTRAINT "Company_pkey" PRIMARY KEY (id);

--
-- Name: Depot Depot_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Depot"
    ADD CONSTRAINT "Depot_pkey" PRIMARY KEY (id);

--
-- Name: GameAgeRating GameAgeRating_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."GameAgeRating"
    ADD CONSTRAINT "GameAgeRating_pkey" PRIMARY KEY (id);

--
-- Name: GameRating GameRating_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."GameRating"
    ADD CONSTRAINT "GameRating_pkey" PRIMARY KEY (id);

--
-- Name: GameTag GameTag_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."GameTag"
    ADD CONSTRAINT "GameTag_pkey" PRIMARY KEY (id);

--
-- Name: GameVersion GameVersion_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."GameVersion"
    ADD CONSTRAINT "GameVersion_pkey" PRIMARY KEY ("versionId");

--
-- Name: Game Game_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Game"
    ADD CONSTRAINT "Game_pkey" PRIMARY KEY (id);

--
-- Name: Invitation Invitation_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Invitation"
    ADD CONSTRAINT "Invitation_pkey" PRIMARY KEY (id);

--
-- Name: LaunchConfiguration LaunchConfiguration_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."LaunchConfiguration"
    ADD CONSTRAINT "LaunchConfiguration_pkey" PRIMARY KEY ("launchId");

--
-- Name: Library Library_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Library"
    ADD CONSTRAINT "Library_pkey" PRIMARY KEY (id);

--
-- Name: LinkedAuthMec LinkedAuthMec_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."LinkedAuthMec"
    ADD CONSTRAINT "LinkedAuthMec_pkey" PRIMARY KEY ("userId", mec);

--
-- Name: LinkedMFAMec LinkedMFAMec_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."LinkedMFAMec"
    ADD CONSTRAINT "LinkedMFAMec_pkey" PRIMARY KEY ("userId", mec);

--
-- Name: NewsTag NewsTag_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."NewsTag"
    ADD CONSTRAINT "NewsTag_pkey" PRIMARY KEY (id);

--
-- Name: Notification Notification_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Notification"
    ADD CONSTRAINT "Notification_pkey" PRIMARY KEY (id);

--
-- Name: ObjectHash ObjectHash_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."ObjectHash"
    ADD CONSTRAINT "ObjectHash_pkey" PRIMARY KEY (id);

--
-- Name: Playtime Playtime_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Playtime"
    ADD CONSTRAINT "Playtime_pkey" PRIMARY KEY ("gameId", "userId");

--
-- Name: SaveSlot SaveSlot_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."SaveSlot"
    ADD CONSTRAINT "SaveSlot_pkey" PRIMARY KEY ("gameId", "userId", index);

--
-- Name: Screenshot Screenshot_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Screenshot"
    ADD CONSTRAINT "Screenshot_pkey" PRIMARY KEY (id);

--
-- Name: Session Session_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Session"
    ADD CONSTRAINT "Session_pkey" PRIMARY KEY (token);

--
-- Name: SetupConfiguration SetupConfiguration_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."SetupConfiguration"
    ADD CONSTRAINT "SetupConfiguration_pkey" PRIMARY KEY ("setupId");

--
-- Name: Task Task_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Task"
    ADD CONSTRAINT "Task_pkey" PRIMARY KEY (id, started);

--
-- Name: UnimportedGameVersion UnimportedGameVersion_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."UnimportedGameVersion"
    ADD CONSTRAINT "UnimportedGameVersion_pkey" PRIMARY KEY (id);

--
-- Name: UserGroup UserGroup_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."UserGroup"
    ADD CONSTRAINT "UserGroup_pkey" PRIMARY KEY (id);

--
-- Name: User User_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."User"
    ADD CONSTRAINT "User_pkey" PRIMARY KEY (id);

--
-- Name: _ArticleToNewsTag _ArticleToNewsTag_AB_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."_ArticleToNewsTag"
    ADD CONSTRAINT "_ArticleToNewsTag_AB_pkey" PRIMARY KEY ("A", "B");

--
-- Name: _GameToGameTag _GameToGameTag_AB_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."_GameToGameTag"
    ADD CONSTRAINT "_GameToGameTag_AB_pkey" PRIMARY KEY ("A", "B");

--
-- Name: _UserToUserGroup _UserToUserGroup_AB_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."_UserToUserGroup"
    ADD CONSTRAINT "_UserToUserGroup_AB_pkey" PRIMARY KEY ("A", "B");

--
-- Name: _developers _developers_AB_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public._developers
    ADD CONSTRAINT "_developers_AB_pkey" PRIMARY KEY ("A", "B");

--
-- Name: _publishers _publishers_AB_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public._publishers
    ADD CONSTRAINT "_publishers_AB_pkey" PRIMARY KEY ("A", "B");

--
-- Name: _requiredContent _requiredContent_AB_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."_requiredContent"
    ADD CONSTRAINT "_requiredContent_AB_pkey" PRIMARY KEY ("A", "B");

--
-- Name: APIToken_token_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX "APIToken_token_idx" ON public."APIToken" USING btree (token);

--
-- Name: APIToken_token_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX "APIToken_token_key" ON public."APIToken" USING btree (token);

--
-- Name: BannedAgeRating_userGroupId_organization_rating_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX "BannedAgeRating_userGroupId_organization_rating_key" ON public."BannedAgeRating" USING btree ("userGroupId", organization, rating);

--
-- Name: Company_metadataSource_metadataId_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX "Company_metadataSource_metadataId_key" ON public."Company" USING btree ("metadataSource", "metadataId");

--
-- Name: GameAgeRating_gameId_organization_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX "GameAgeRating_gameId_organization_key" ON public."GameAgeRating" USING btree ("gameId", organization);

--
-- Name: GameRating_metadataSource_metadataId_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX "GameRating_metadataSource_metadataId_key" ON public."GameRating" USING btree ("metadataSource", "metadataId");

--
-- Name: GameTag_name_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX "GameTag_name_idx" ON public."GameTag" USING gist (name public.gist_trgm_ops (siglen='32'));

--
-- Name: GameTag_name_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX "GameTag_name_key" ON public."GameTag" USING btree (name);

--
-- Name: GameVersion_gameId_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX "GameVersion_gameId_idx" ON public."GameVersion" USING btree ("gameId");

--
-- Name: Game_libraryId_libraryPath_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX "Game_libraryId_libraryPath_key" ON public."Game" USING btree ("libraryId", "libraryPath");

--
-- Name: Game_mName_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX "Game_mName_idx" ON public."Game" USING gist ("mName" public.gist_trgm_ops (siglen='32'));

--
-- Name: Game_metadataSource_metadataId_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX "Game_metadataSource_metadataId_key" ON public."Game" USING btree ("metadataSource", "metadataId");

--
-- Name: NewsTag_name_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX "NewsTag_name_key" ON public."NewsTag" USING btree (name);

--
-- Name: Notification_userId_nonce_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX "Notification_userId_nonce_key" ON public."Notification" USING btree ("userId", nonce);

--
-- Name: Playtime_userId_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX "Playtime_userId_idx" ON public."Playtime" USING btree ("userId");

--
-- Name: Screenshot_gameId_userId_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX "Screenshot_gameId_userId_idx" ON public."Screenshot" USING btree ("gameId", "userId");

--
-- Name: Screenshot_userId_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX "Screenshot_userId_idx" ON public."Screenshot" USING btree ("userId");

--
-- Name: UnimportedGameVersion_gameId_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX "UnimportedGameVersion_gameId_idx" ON public."UnimportedGameVersion" USING btree ("gameId");

--
-- Name: UserGroup_name_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX "UserGroup_name_key" ON public."UserGroup" USING btree (name);

--
-- Name: User_username_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX "User_username_key" ON public."User" USING btree (username);

--
-- Name: _ArticleToNewsTag_B_index; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX "_ArticleToNewsTag_B_index" ON public."_ArticleToNewsTag" USING btree ("B");

--
-- Name: _GameToGameTag_B_index; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX "_GameToGameTag_B_index" ON public."_GameToGameTag" USING btree ("B");

--
-- Name: _UserToUserGroup_B_index; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX "_UserToUserGroup_B_index" ON public."_UserToUserGroup" USING btree ("B");

--
-- Name: _developers_B_index; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX "_developers_B_index" ON public._developers USING btree ("B");

--
-- Name: _publishers_B_index; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX "_publishers_B_index" ON public._publishers USING btree ("B");

--
-- Name: _requiredContent_B_index; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX "_requiredContent_B_index" ON public."_requiredContent" USING btree ("B");

--
-- Name: APIToken APIToken_clientId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."APIToken"
    ADD CONSTRAINT "APIToken_clientId_fkey" FOREIGN KEY ("clientId") REFERENCES public."Client"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: APIToken APIToken_userId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."APIToken"
    ADD CONSTRAINT "APIToken_userId_fkey" FOREIGN KEY ("userId") REFERENCES public."User"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: Article Article_authorId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Article"
    ADD CONSTRAINT "Article_authorId_fkey" FOREIGN KEY ("authorId") REFERENCES public."User"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: BannedAgeRating BannedAgeRating_userGroupId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."BannedAgeRating"
    ADD CONSTRAINT "BannedAgeRating_userGroupId_fkey" FOREIGN KEY ("userGroupId") REFERENCES public."UserGroup"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: Client Client_userId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Client"
    ADD CONSTRAINT "Client_userId_fkey" FOREIGN KEY ("userId") REFERENCES public."User"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: CollectionEntry CollectionEntry_collectionId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."CollectionEntry"
    ADD CONSTRAINT "CollectionEntry_collectionId_fkey" FOREIGN KEY ("collectionId") REFERENCES public."Collection"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: CollectionEntry CollectionEntry_gameId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."CollectionEntry"
    ADD CONSTRAINT "CollectionEntry_gameId_fkey" FOREIGN KEY ("gameId") REFERENCES public."Game"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: Collection Collection_userId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Collection"
    ADD CONSTRAINT "Collection_userId_fkey" FOREIGN KEY ("userId") REFERENCES public."User"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: GameAgeRating GameAgeRating_gameId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."GameAgeRating"
    ADD CONSTRAINT "GameAgeRating_gameId_fkey" FOREIGN KEY ("gameId") REFERENCES public."Game"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: GameRating GameRating_gameId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."GameRating"
    ADD CONSTRAINT "GameRating_gameId_fkey" FOREIGN KEY ("gameId") REFERENCES public."Game"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: GameVersion GameVersion_gameId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."GameVersion"
    ADD CONSTRAINT "GameVersion_gameId_fkey" FOREIGN KEY ("gameId") REFERENCES public."Game"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: Game Game_libraryId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Game"
    ADD CONSTRAINT "Game_libraryId_fkey" FOREIGN KEY ("libraryId") REFERENCES public."Library"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: LaunchConfiguration LaunchConfiguration_emulatorId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."LaunchConfiguration"
    ADD CONSTRAINT "LaunchConfiguration_emulatorId_fkey" FOREIGN KEY ("emulatorId") REFERENCES public."LaunchConfiguration"("launchId") ON UPDATE CASCADE ON DELETE SET NULL;

--
-- Name: LaunchConfiguration LaunchConfiguration_versionId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."LaunchConfiguration"
    ADD CONSTRAINT "LaunchConfiguration_versionId_fkey" FOREIGN KEY ("versionId") REFERENCES public."GameVersion"("versionId") ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: LinkedAuthMec LinkedAuthMec_userId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."LinkedAuthMec"
    ADD CONSTRAINT "LinkedAuthMec_userId_fkey" FOREIGN KEY ("userId") REFERENCES public."User"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: LinkedMFAMec LinkedMFAMec_userId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."LinkedMFAMec"
    ADD CONSTRAINT "LinkedMFAMec_userId_fkey" FOREIGN KEY ("userId") REFERENCES public."User"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: Notification Notification_userId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Notification"
    ADD CONSTRAINT "Notification_userId_fkey" FOREIGN KEY ("userId") REFERENCES public."User"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: Playtime Playtime_gameId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Playtime"
    ADD CONSTRAINT "Playtime_gameId_fkey" FOREIGN KEY ("gameId") REFERENCES public."Game"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: Playtime Playtime_userId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Playtime"
    ADD CONSTRAINT "Playtime_userId_fkey" FOREIGN KEY ("userId") REFERENCES public."User"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: SaveSlot SaveSlot_gameId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."SaveSlot"
    ADD CONSTRAINT "SaveSlot_gameId_fkey" FOREIGN KEY ("gameId") REFERENCES public."Game"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: SaveSlot SaveSlot_lastUsedClientId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."SaveSlot"
    ADD CONSTRAINT "SaveSlot_lastUsedClientId_fkey" FOREIGN KEY ("lastUsedClientId") REFERENCES public."Client"(id) ON UPDATE CASCADE ON DELETE SET NULL;

--
-- Name: SaveSlot SaveSlot_userId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."SaveSlot"
    ADD CONSTRAINT "SaveSlot_userId_fkey" FOREIGN KEY ("userId") REFERENCES public."User"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: Screenshot Screenshot_gameId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Screenshot"
    ADD CONSTRAINT "Screenshot_gameId_fkey" FOREIGN KEY ("gameId") REFERENCES public."Game"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: Screenshot Screenshot_userId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Screenshot"
    ADD CONSTRAINT "Screenshot_userId_fkey" FOREIGN KEY ("userId") REFERENCES public."User"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: Session Session_userId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."Session"
    ADD CONSTRAINT "Session_userId_fkey" FOREIGN KEY ("userId") REFERENCES public."User"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: SetupConfiguration SetupConfiguration_versionId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."SetupConfiguration"
    ADD CONSTRAINT "SetupConfiguration_versionId_fkey" FOREIGN KEY ("versionId") REFERENCES public."GameVersion"("versionId") ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: UnimportedGameVersion UnimportedGameVersion_gameId_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."UnimportedGameVersion"
    ADD CONSTRAINT "UnimportedGameVersion_gameId_fkey" FOREIGN KEY ("gameId") REFERENCES public."Game"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: _ArticleToNewsTag _ArticleToNewsTag_A_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."_ArticleToNewsTag"
    ADD CONSTRAINT "_ArticleToNewsTag_A_fkey" FOREIGN KEY ("A") REFERENCES public."Article"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: _ArticleToNewsTag _ArticleToNewsTag_B_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."_ArticleToNewsTag"
    ADD CONSTRAINT "_ArticleToNewsTag_B_fkey" FOREIGN KEY ("B") REFERENCES public."NewsTag"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: _GameToGameTag _GameToGameTag_A_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."_GameToGameTag"
    ADD CONSTRAINT "_GameToGameTag_A_fkey" FOREIGN KEY ("A") REFERENCES public."Game"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: _GameToGameTag _GameToGameTag_B_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."_GameToGameTag"
    ADD CONSTRAINT "_GameToGameTag_B_fkey" FOREIGN KEY ("B") REFERENCES public."GameTag"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: _UserToUserGroup _UserToUserGroup_A_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."_UserToUserGroup"
    ADD CONSTRAINT "_UserToUserGroup_A_fkey" FOREIGN KEY ("A") REFERENCES public."User"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: _UserToUserGroup _UserToUserGroup_B_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."_UserToUserGroup"
    ADD CONSTRAINT "_UserToUserGroup_B_fkey" FOREIGN KEY ("B") REFERENCES public."UserGroup"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: _developers _developers_A_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public._developers
    ADD CONSTRAINT "_developers_A_fkey" FOREIGN KEY ("A") REFERENCES public."Company"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: _developers _developers_B_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public._developers
    ADD CONSTRAINT "_developers_B_fkey" FOREIGN KEY ("B") REFERENCES public."Game"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: _publishers _publishers_A_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public._publishers
    ADD CONSTRAINT "_publishers_A_fkey" FOREIGN KEY ("A") REFERENCES public."Company"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: _publishers _publishers_B_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public._publishers
    ADD CONSTRAINT "_publishers_B_fkey" FOREIGN KEY ("B") REFERENCES public."Game"(id) ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: _requiredContent _requiredContent_A_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."_requiredContent"
    ADD CONSTRAINT "_requiredContent_A_fkey" FOREIGN KEY ("A") REFERENCES public."GameVersion"("versionId") ON UPDATE CASCADE ON DELETE CASCADE;

--
-- Name: _requiredContent _requiredContent_B_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."_requiredContent"
    ADD CONSTRAINT "_requiredContent_B_fkey" FOREIGN KEY ("B") REFERENCES public."GameVersion"("versionId") ON UPDATE CASCADE ON DELETE CASCADE;

--
--

