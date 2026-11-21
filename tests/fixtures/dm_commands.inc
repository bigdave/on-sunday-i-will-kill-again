/* GPL-2.0-or-later. Local tests only, inside classic ServerCommand().
 * Only adds console fixture controls; damage/scoring/team rules stay unchanged.
 */
    if (!Q_stricmp(cmd,"dmcheck_health") || !Q_stricmp(cmd,"dmcheck_loadout") || !Q_stricmp(cmd,"dmcheck_pair")) {
        int i; edict_t *e=NULL;
        for (i=1;i<=maxclients->value;i++)
            if (g_edicts[i].inuse && g_edicts[i].client && !Q_stricmp(g_edicts[i].client->pers.netname,gi.argv(2))) {e=&g_edicts[i];break;}
        if (!e || e->health<=0) return;
        if (!Q_stricmp(cmd,"dmcheck_health") && gi.argc()==4) {
            e->health=atoi(gi.argv(3));
        } else if (!Q_stricmp(cmd,"dmcheck_loadout") && gi.argc()==4) {
            const gitem_t *w=FindItem(gi.argv(3)), *ammo;
            if (!w) return;
            for(i=0;i<MAX_ITEMS;i++) e->client->pers.inventory[i]=0;
            e->client->pers.inventory[ITEM_INDEX(FindItem("Blaster"))]=1;
            e->client->pers.inventory[ITEM_INDEX(w)]=1;
            ammo=w->ammo?FindItem(w->ammo):NULL;
            if(ammo)e->client->pers.inventory[ITEM_INDEX(ammo)]=200;
            e->client->newweapon=w;ChangeWeapon(e);
        } else if (!Q_stricmp(cmd,"dmcheck_pair") && gi.argc()==4) {
            edict_t *target=NULL,*spot=NULL;int a,j;vec3_t from,pos,down;trace_t tr,ground;
            for(i=1;i<=maxclients->value;i++)
                if(g_edicts[i].inuse && g_edicts[i].client && !Q_stricmp(g_edicts[i].client->pers.netname,gi.argv(3))) {target=&g_edicts[i];break;}
            if(!target || target->health<=0)return;
            while((spot=G_Find(spot,FOFS(classname),"info_player_deathmatch"))!=NULL) {
              VectorCopy(spot->s.origin,from);from[2]+=10;VectorCopy(from,down);down[2]-=128;
              ground=gi.trace(from,e->mins,e->maxs,down,e,MASK_PLAYERSOLID);
              if(ground.startsolid || ground.fraction==1)continue;
              VectorCopy(ground.endpos,from);from[2]+=.125;
              for(a=0;a<8;a++) {
                VectorCopy(from,pos);pos[0]+=128*cos(a*M_PI/4);pos[1]+=128*sin(a*M_PI/4);
                tr=gi.trace(from,e->mins,e->maxs,pos,e,MASK_PLAYERSOLID);
                if(tr.startsolid || tr.fraction<1)continue;
                VectorCopy(pos,down);down[2]-=64;
                ground=gi.trace(pos,target->mins,target->maxs,down,target,MASK_PLAYERSOLID);
                if(ground.startsolid || ground.fraction==1 || fabs(ground.endpos[2]-from[2])>16)continue;
                for(j=0;j<2;j++) {
                  edict_t *player=j?target:e;vec3_t where;
                  VectorCopy(j?ground.endpos:from,where);where[2]+=.125;
                  VectorCopy(where,player->s.origin);VectorCopy(where,player->s.old_origin);VectorClear(player->velocity);
                  for(i=0;i<3;i++){player->client->ps.pmove.origin[i]=(short)(where[i]*8);player->client->ps.pmove.velocity[i]=0;}
                  player->groundentity=NULL;player->client->ps.pmove.pm_flags=0;player->client->ps.pmove.pm_time=0;gi.linkentity(player);
                }
                /* Suppress map pickups so health assertions cannot be changed by a health pack. */
                for(i=maxclients->value+1;i<globals.num_edicts;i++) {
                    edict_t *item=&g_edicts[i];
                    if(item->inuse && item->item && item->solid==SOLID_TRIGGER){item->solid=SOLID_NOT;gi.linkentity(item);}
                }
                gi.bprintf(PRINT_HIGH,"DMCHECK paired %s %s\n",e->client->pers.netname,target->client->pers.netname);return;
              }
            }
            gi.bprintf(PRINT_HIGH,"DMCHECK pair failed\n");
        }
        return;
    }
