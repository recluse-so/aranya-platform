impl<EN, SP, CE> Actions<EN, SP, CE> for ActionsImpl<EN, SP, CE>
where
    /* type constraints */
{
    // Add new SSH-specific methods
    
    /// Grants SSH access to a user for a specific host
    fn grant_ssh_access(&self, 
                       user_id: UserId, 
                       host: String,
                       access_level: SshAccessLevel
    ) -> impl Future<Output = Result<Vec<Effect>>> + Send {
        self.with_actor(move |actor| {
            actor.grant_ssh_access(user_id.into(), host, access_level)?;
            Ok(())
        })
    }
    
    /// Revokes SSH access from a user for a specific host
    fn revoke_ssh_access(&self,
                        user_id: UserId,
                        host: String
    ) -> impl Future<Output = Result<Vec<Effect>>> + Send {
        self.with_actor(move |actor| {
            actor.revoke_ssh_access(user_id.into(), host)?;
            Ok(())
        })
    }
}