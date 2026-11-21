    /* GPL-2.0-or-later. LOCAL TEST FIXTURE ONLY. Never use on a public server. */
    if (!Q_stricmp(cmd, "ctfcheck_place") || !Q_stricmp(cmd, "ctfcheck_flag") || !Q_stricmp(cmd, "ctfcheck_health") || !Q_stricmp(cmd, "ctfcheck_loadout")) {
        int i; edict_t *e = NULL;
        for (i=1;i<=maxclients->value;i++) {
            if (g_edicts[i].inuse && g_edicts[i].client && !Q_stricmp(g_edicts[i].client->pers.netname,gi.argv(2))) {e=&g_edicts[i];break;}
        }
        if (!e || e->health<=0) return;
        if (!Q_stricmp(cmd,"ctfcheck_loadout") && gi.argc()==4) {
            const char *name=gi.argv(3);gitem_t *w=FindItem(name),*ammo;
            if (!w) return;
            for (i=0;i<MAX_ITEMS;i++) e->client->pers.inventory[i]=0;
            e->client->pers.inventory[ITEM_INDEX(FindItem("Blaster"))]=1;
            e->client->pers.inventory[ITEM_INDEX(w)]=1;
            ammo=w->ammo?FindItem(w->ammo):NULL;
            if (ammo) e->client->pers.inventory[ITEM_INDEX(ammo)]=200;
            e->client->newweapon=w;ChangeWeapon(e);
            gi.bprintf(PRINT_HIGH,"CTFCHECK loadout %s %s\n",e->client->pers.netname,w->pickup_name);
        } else if (!Q_stricmp(cmd,"ctfcheck_health") && gi.argc()==4) {
            e->health=atoi(gi.argv(3));e->client->ps.stats[STAT_HEALTH]=e->health;
            gi.bprintf(PRINT_HIGH,"CTFCHECK health set for %s\n",e->client->pers.netname);
        } else if (!Q_stricmp(cmd,"ctfcheck_place") && gi.argc()==6) {
            for (i=0;i<3;i++) {e->s.origin[i]=atof(gi.argv(3+i));e->s.old_origin[i]=e->s.origin[i];e->velocity[i]=0;e->client->ps.pmove.origin[i]=(short)(e->s.origin[i]*8);e->client->ps.pmove.velocity[i]=0;}
            e->client->ps.pmove.pm_flags=0;e->client->ps.pmove.pm_time=0;e->groundentity=NULL;gi.linkentity(e);
            gi.bprintf(PRINT_HIGH,"CTFCHECK placed %s\n",e->client->pers.netname);
        } else if (!Q_stricmp(cmd,"ctfcheck_flag")) {
            edict_t *flag=NULL;char *classname=e->client->resp.ctf_team==CTF_TEAM1?"item_flag_team2":"item_flag_team1";
            flag=G_Find(NULL,FOFS(classname),classname);
            if (flag) Touch_Item(flag,e,NULL,NULL);
            gi.bprintf(PRINT_HIGH,"CTFCHECK flag granted through Touch_Item to %s\n",e->client->pers.netname);
        }
        return;
    }
